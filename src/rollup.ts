import type { TokenCounts, UsageEvent } from "./types.ts";

export type RollupBy = "model" | "day" | "session" | "harness" | "workspace" | "block";

// the default billing-block width: five hours in milliseconds
export const DEFAULT_BLOCK_MS = 18_000_000;

export type RollupOptions = {
  by: RollupBy;
  // IANA timezone for day bucketing and block-start hour flooring; defaults to
  // the system timezone
  tz?: string;
  // billing-block width in milliseconds (by === "block" only); a missing or
  // non-positive value uses DEFAULT_BLOCK_MS
  blockMs?: number;
};

export type Rollup = {
  key: string;
  tokens: TokenCounts;
  events: number;
  turns: number;
  // underlying model calls; may exceed events for session-aggregate sources
  // (each event contributes its reported finite count, else 1)
  calls: number;
  costUsd?: number;
};

function emptyTokens(): TokenCounts {
  return { input: 0, output: 0, cacheRead: 0, cacheWrite: 0, reasoning: 0 };
}

function dayKey(timestamp: string, tz: string | undefined): string {
  const date = new Date(timestamp);
  if (Number.isNaN(date.getTime())) return "invalid-date";
  // en-CA formats as YYYY-MM-DD
  const options: Intl.DateTimeFormatOptions = {
    year: "numeric",
    month: "2-digit",
    day: "2-digit",
  };
  try {
    const format = new Intl.DateTimeFormat("en-CA", {
      ...(tz !== undefined ? { timeZone: tz } : {}),
      ...options,
    });
    return format.format(date);
  } catch {
    // an unknown zone degrades to the UTC calendar date rather than aborting
    // the rollup, matching the native core
    return new Intl.DateTimeFormat("en-CA", { timeZone: "UTC", ...options }).format(date);
  }
}

export function rollupKey(event: UsageEvent, options: RollupOptions): string {
  switch (options.by) {
    case "model":
      return event.model;
    case "day":
      return dayKey(event.timestamp, options.tz);
    case "session":
      return event.sessionId;
    case "harness":
      return event.harness;
    case "workspace":
      return event.workspace ?? "(unknown)";
    // a block's key is derived from event ordering, not one event; the keyed
    // loop never calls this for "block"
    case "block":
      return dayKey(event.timestamp, options.tz);
  }
}

// accumulate one event's counters into the bucket; shared by the keyed and
// block rollup paths so both bill tokens/calls/cost identically
function accumulate(entry: Rollup, event: UsageEvent): void {
  entry.tokens.input += event.tokens.input;
  entry.tokens.output += event.tokens.output;
  entry.tokens.cacheRead += event.tokens.cacheRead;
  entry.tokens.cacheWrite += event.tokens.cacheWrite;
  if (event.tokens.cacheWrite1h !== undefined) {
    // clamp each event's 1h split into [0, its own cacheWrite] before
    // summing, so one malformed event cannot rebill other events' writes
    // at the 1h rate; non-finite splits are treated as 0
    const reported1h = Number.isFinite(event.tokens.cacheWrite1h) ? event.tokens.cacheWrite1h : 0;
    const clamped1h = Math.min(Math.max(reported1h, 0), Math.max(event.tokens.cacheWrite, 0));
    entry.tokens.cacheWrite1h = (entry.tokens.cacheWrite1h ?? 0) + clamped1h;
  }
  entry.tokens.reasoning += event.tokens.reasoning;
  entry.events += 1;
  if (event.turn) entry.turns += 1;
  // an event with no reported call count still represents at least one call;
  // a reported count must be finite or it is treated as absent so
  // NaN/Infinity cannot poison rollups and downstream pricing
  const reported = event.calls;
  entry.calls += reported !== undefined && Number.isFinite(reported) ? Math.max(0, reported) : 1;
  const recorded = event.costUsd;
  if (recorded !== undefined && Number.isFinite(recorded)) {
    entry.costUsd = (entry.costUsd ?? 0) + recorded;
  }
}

function newBucket(key: string): Rollup {
  return { key, tokens: emptyTokens(), events: 0, turns: 0, calls: 0 };
}

// days since 1970-01-01 for a civil (proleptic Gregorian) date; Howard
// Hinnant's days_from_civil, mirrored from readers::shared in the core so the
// rollup boundary parses timestamps with the SAME grammar on both sides
function daysFromCivil(year: number, month: number, day: number): number {
  const y = month <= 2 ? year - 1 : year;
  const era = Math.floor((y >= 0 ? y : y - 399) / 400);
  const yoe = y - era * 400;
  const m = month;
  const doy = Math.floor((153 * (m > 2 ? m - 3 : m + 9) + 2) / 5) + day - 1;
  const doe = yoe * 365 + Math.floor(yoe / 4) - Math.floor(yoe / 100) + doy;
  return era * 146_097 + doe - 719_468;
}

// parse an ISO-8601 date-time string to epoch millis using the SAME strict
// grammar as the Rust core's date_parse_ms (ECMA-262 Date Time String Format:
// YYYY-MM-DD (T HH:mm(:ss(.sss)?)?)? (Z | ±HH:mm)?, offsetless date-time is
// UTC). Returns undefined for anything outside it, so both sides bucket the
// same non-canonical or offsetless input into the invalid-date block rather
// than letting host Date leniency diverge. Mirrors date_parse_ms in rollup.rs
// via readers::shared.
function dateParseMs(input: string): number | undefined {
  const isDigits = (from: number, len: number): number | undefined => {
    const slice = input.slice(from, from + len);
    if (slice.length !== len || !/^[0-9]+$/.test(slice)) return undefined;
    return Number(slice);
  };
  if (input.length < 10 || input[4] !== "-" || input[7] !== "-") return undefined;
  const year = isDigits(0, 4);
  const month = isDigits(5, 2);
  const day = isDigits(8, 2);
  if (year === undefined || month === undefined || day === undefined) return undefined;
  if (month < 1 || month > 12 || day < 1 || day > 31) return undefined;
  let hour = 0;
  let minute = 0;
  let second = 0;
  let milli = 0;
  let offsetMin = 0;
  if (input.length > 10) {
    if (input[10] !== "T" && input[10] !== " ") return undefined;
    if (input.length < 16 || input[13] !== ":") return undefined;
    const h = isDigits(11, 2);
    const mi = isDigits(14, 2);
    if (h === undefined || mi === undefined) return undefined;
    hour = h;
    minute = mi;
    let idx = 16;
    if (input.length > idx && input[idx] === ":") {
      const sec = isDigits(17, 2);
      if (sec === undefined) return undefined;
      second = sec;
      idx = 19;
      if (input.length > idx && input[idx] === ".") {
        const fracStart = idx + 1;
        let fracEnd = fracStart;
        while (fracEnd < input.length && /[0-9]/.test(input[fracEnd] ?? "")) fracEnd += 1;
        if (fracEnd === fracStart) return undefined;
        const secondsFrac = Number(`0.${input.slice(fracStart, fracEnd)}`);
        milli = Math.round(secondsFrac * 1000);
        idx = fracEnd;
      }
    }
    if (input.length > idx) {
      const c = input[idx];
      if (c === "Z" && input.length === idx + 1) {
        // UTC
      } else if (c === "+" || c === "-") {
        const sign = c === "-" ? -1 : 1;
        if (input.length < idx + 6 || input[idx + 3] !== ":") return undefined;
        const oh = isDigits(idx + 1, 2);
        const om = isDigits(idx + 4, 2);
        if (oh === undefined || om === undefined) return undefined;
        offsetMin = sign * (oh * 60 + om);
      } else {
        return undefined;
      }
    }
  }
  if (hour < 0 || hour > 23) return undefined;
  if (minute < 0 || minute > 59) return undefined;
  if (second < 0 || second > 59) return undefined;
  if (milli < 0 || milli > 999) return undefined;
  const days = daysFromCivil(year, month, day);
  return (
    days * 86_400_000 +
    hour * 3_600_000 +
    minute * 60_000 +
    second * 1_000 +
    milli -
    offsetMin * 60_000
  );
}

const HOUR_MS = 3_600_000;

function floorUtcHour(ms: number): number {
  return ms - (((ms % HOUR_MS) + HOUR_MS) % HOUR_MS);
}

// the epoch millisecond of ms floored to the start of its clock hour in tz.
// Explicit "UTC" floors via direct arithmetic; an omitted tz resolves the
// system zone (the same path dayKey uses: Intl with no timeZone option) and a
// named zone floors the local wall-clock hour (honoring fractional-hour
// offsets) and converts back to the instant. Mirrors floor_to_hour_ms in
// rollup.rs, which routes an omitted zone to TimeZone::system().
function floorToHourMs(ms: number, tz: string | undefined): number {
  if (tz === "UTC") return floorUtcHour(ms);
  try {
    const format = new Intl.DateTimeFormat("en-US", {
      ...(tz !== undefined ? { timeZone: tz } : {}),
      hour12: false,
      year: "numeric",
      month: "2-digit",
      day: "2-digit",
      hour: "2-digit",
      minute: "2-digit",
      second: "2-digit",
    });
    const parts: Record<string, string> = {};
    for (const p of format.formatToParts(new Date(ms))) parts[p.type] = p.value;
    // the zone's minute/second past the wall-clock hour, in ms, plus the
    // sub-second remainder of the instant, is the amount to subtract to reach
    // the local hour start
    const minute = Number(parts.minute === "24" ? "0" : parts.minute);
    const second = Number(parts.second);
    const subSecond = ((ms % 1000) + 1000) % 1000;
    const withinHour = minute * 60_000 + second * 1000 + subSecond;
    return ms - withinHour;
  } catch {
    return floorUtcHour(ms);
  }
}

// aggregate events into session-window billing blocks. Events are ordered by
// timestamp; a block opens at the first event's timestamp floored to the hour
// in tz and spans blockMs. Events in [start, start + blockMs) join the block;
// the first event past that opens a new one. The row key is the ISO instant of
// the block start. Mirrors block_rollup in rollup.rs.
function blockRollup(events: UsageEvent[], options: RollupOptions): Rollup[] {
  const blockMs =
    options.blockMs !== undefined && Number.isFinite(options.blockMs) && options.blockMs > 0
      ? options.blockMs
      : DEFAULT_BLOCK_MS;
  const tz = options.tz;

  // order by parsed timestamp; unparseable timestamps sort last and share a
  // single "invalid-date" block so a bad row never derails valid windows. The
  // strict dateParseMs (not host Date) gives this boundary the same grammar as
  // the Rust core, so a non-canonical or offsetless timestamp buckets to
  // invalid-date on BOTH sides rather than diverging on host leniency
  const withMs = events.map((e) => {
    return { event: e, ms: dateParseMs(e.timestamp) };
  });
  withMs.sort((a, b) => {
    if (a.ms === undefined) return b.ms === undefined ? 0 : 1;
    if (b.ms === undefined) return -1;
    return a.ms - b.ms;
  });

  const out: Rollup[] = [];
  let current: Rollup | undefined;
  let blockStartMs: number | undefined;
  for (const { event, ms } of withMs) {
    if (ms === undefined) {
      if (current === undefined || current.key !== "invalid-date") {
        current = newBucket("invalid-date");
        out.push(current);
      }
      accumulate(current, event);
      continue;
    }
    const inOpenBlock = blockStartMs !== undefined && ms < blockStartMs + blockMs;
    if (!inOpenBlock || current === undefined) {
      blockStartMs = floorToHourMs(ms, tz);
      current = newBucket(new Date(blockStartMs).toISOString());
      out.push(current);
    }
    accumulate(current, event);
  }
  return out;
}

export function rollup(events: UsageEvent[], options: RollupOptions): Rollup[] {
  if (options.by === "block") return blockRollup(events, options);

  const map = new Map<string, Rollup>();
  for (const event of events) {
    const key = rollupKey(event, options);
    let entry = map.get(key);
    if (entry === undefined) {
      entry = newBucket(key);
      map.set(key, entry);
    }
    accumulate(entry, event);
  }
  return [...map.values()].sort((a, b) => (a.key < b.key ? -1 : a.key > b.key ? 1 : 0));
}
