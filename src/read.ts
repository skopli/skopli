import { createDiagnostics } from "./diagnostics.ts";
import { detectHarnesses, type Detection } from "./detect.ts";
import { native } from "./native.ts";
import {
  createPathResolver,
  effectiveCwd,
  effectiveEnv,
  effectiveHome,
  type PathResolver,
  type PathResolverOptions,
} from "./paths.ts";
import type { Diagnostic, Harness, UsageEvent } from "./types.ts";

export type ReadUsageOptions = PathResolverOptions & {
  harnesses?: Harness[];
  // ISO date or Date; events strictly before are dropped
  // date-only strings (YYYY-MM-DD) mean midnight of that day in tz
  since?: Date | string;
  // ISO date or Date; events at/after are dropped
  // date-only strings (YYYY-MM-DD) mean the exclusive end of that day in tz
  until?: Date | string;
  // IANA timezone for date-only since/until bounds; defaults to the system timezone
  tz?: string;
  subagents?: "include" | "exclude";
};

export type ReadUsageResult = {
  events: UsageEvent[];
  diagnostics: Diagnostic[];
  // per-harness source files (or file:line) skipped as unreadable/malformed
  skipped: Partial<Record<Harness, string[]>>;
};

const DATE_ONLY = /^(\d{4})-(\d{2})-(\d{2})$/;
const DAY_MS = 86_400_000;

function tzOffsetMs(tz: string | undefined, at: number): number {
  const options: Intl.DateTimeFormatOptions = {
    era: "short",
    year: "numeric",
    month: "2-digit",
    day: "2-digit",
    hour: "2-digit",
    minute: "2-digit",
    second: "2-digit",
    hourCycle: "h23",
  };
  // an unknown zone degrades to UTC rather than aborting the read, matching the
  // native core's date-only bound resolution
  let parts: Intl.DateTimeFormatPart[];
  try {
    parts = new Intl.DateTimeFormat("en-CA", {
      ...(tz !== undefined ? { timeZone: tz } : {}),
      ...options,
    }).formatToParts(at);
  } catch {
    parts = new Intl.DateTimeFormat("en-CA", { timeZone: "UTC", ...options }).formatToParts(at);
  }
  const get = (type: Intl.DateTimeFormatPartTypes): number => {
    const part = parts.find((p) => p.type === type);
    if (part === undefined) throw new RangeError(`missing ${type} in formatted date`);
    return Number(part.value);
  };
  const era = parts.find((p) => p.type === "era")?.value ?? "";
  const year = era.startsWith("B") ? 1 - get("year") : get("year");
  const asUtc = new Date(0);
  asUtc.setUTCFullYear(year, get("month") - 1, get("day"));
  asUtc.setUTCHours(get("hour"), get("minute"), get("second"));
  return asUtc.getTime() - at;
}

// resolve a wall-clock midnight in tz to an epoch by sampling nearby offsets;
// an ambiguous midnight (DST overlap) takes its first occurrence, and a
// midnight inside a DST gap takes the first valid instant of the day
function zonedMidnight(
  value: string,
  match: RegExpExecArray,
  tz: string | undefined,
  dayOffset: number,
): number {
  const year = Number(match[1]);
  const month = Number(match[2]);
  const day = Number(match[3]);
  const roundtrip = new Date(0);
  roundtrip.setUTCFullYear(year, month - 1, day);
  const base = roundtrip.getTime();
  if (
    roundtrip.getUTCFullYear() !== year ||
    roundtrip.getUTCMonth() !== month - 1 ||
    roundtrip.getUTCDate() !== day
  ) {
    throw new RangeError(`invalid date: ${value}`);
  }
  const guess = base + dayOffset * DAY_MS;
  const offsets = new Set([
    tzOffsetMs(tz, guess - DAY_MS),
    tzOffsetMs(tz, guess),
    tzOffsetMs(tz, guess + DAY_MS),
  ]);
  const candidates = [...offsets].map((offset) => guess - offset);
  const valid = candidates.filter((candidate) => candidate + tzOffsetMs(tz, candidate) === guess);
  if (valid.length > 0) return Math.min(...valid);
  return Math.max(...candidates);
}

function toTime(
  value: Date | string | undefined,
  tz: string | undefined,
  exclusiveEndOfDay: boolean,
): number | null {
  if (value === undefined) return null;
  if (typeof value === "string") {
    const match = DATE_ONLY.exec(value);
    if (match !== null) return zonedMidnight(value, match, tz, exclusiveEndOfDay ? 1 : 0);
  }
  const time = value instanceof Date ? value.getTime() : Date.parse(value);
  if (!Number.isFinite(time)) throw new RangeError(`invalid date: ${String(value)}`);
  return time;
}

export function detect(options: PathResolverOptions = {}): Detection {
  return detectHarnesses(createPathResolver(options));
}

// The addon cannot see the parent Node process env, so the facade resolves the
// effective {home, env} on the TS side and passes them down as concrete data.
async function gatherHarness(
  harness: Harness,
  options: ReadUsageOptions,
): Promise<{ events: UsageEvent[]; diagnostics: Diagnostic[]; skipped: string[] }> {
  // empty-string values are dropped so the addon's "empty means unset" rule
  // agrees with the resolver
  const env: Record<string, string> = {};
  for (const [name, value] of Object.entries(effectiveEnv(options))) {
    if (value !== undefined && value !== "") env[name] = value;
  }
  const optionsJson = JSON.stringify({
    home: effectiveHome(options),
    env,
    cwd: effectiveCwd(options),
  });
  const raw = await native().readHarness(harness, optionsJson);
  return JSON.parse(raw) as {
    events: UsageEvent[];
    diagnostics: Diagnostic[];
    skipped: string[];
  };
}

export async function readUsage(options: ReadUsageOptions = {}): Promise<ReadUsageResult> {
  const resolver: PathResolver = createPathResolver(options);
  const since = toTime(options.since, options.tz, false);
  const until = toTime(options.until, options.tz, true);
  const excludeSubagents = options.subagents === "exclude";
  const harnesses = options.harnesses ?? detectHarnesses(resolver).supported;

  const diagnostics = createDiagnostics();
  const events: UsageEvent[] = [];
  const skipped: Partial<Record<Harness, string[]>> = {};

  for (const harness of harnesses) {
    const result = await gatherHarness(harness, options);
    // reader warnings are surfaced before this harness's per-event filtering,
    // matching the original in-process order (warn sink fires during the read)
    for (const diagnostic of result.diagnostics)
      diagnostics.add(diagnostic.severity, diagnostic.message, diagnostic);
    if (result.skipped.length > 0) skipped[harness] = result.skipped;
    for (const event of result.events) {
      // one timestamp policy regardless of filtering: an unparsable timestamp
      // drops the event with a diagnostic, never a silent pass-through
      const at = Date.parse(event.timestamp);
      if (Number.isNaN(at)) {
        diagnostics.warn(
          `dropping event ${event.messageId} with unusable timestamp ${JSON.stringify(event.timestamp)}`,
          { harness },
        );
        continue;
      }
      if (since !== null && at < since) continue;
      if (until !== null && at >= until) continue;
      if (excludeSubagents && event.subagent) continue;
      events.push(event);
    }
  }

  return { events, diagnostics: diagnostics.list(), skipped };
}
