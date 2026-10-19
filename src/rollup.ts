import type { TokenCounts, UsageEvent } from "./types.ts";

export type RollupBy = "model" | "day" | "session" | "harness" | "workspace" | "block";

// the default billing-block width: five hours in milliseconds
export const DEFAULT_BLOCK_MS = 18_000_000;

export type RollupOptions = {
  by: RollupBy;
  // IANA timezone for day bucketing and block-start flooring; defaults to
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
function emptyRollup(key: string): Rollup {
  return {
    key,
    tokens: { input: 0, output: 0, cacheRead: 0, cacheWrite: 0, reasoning: 0 },
    events: 0,
    turns: 0,
    calls: 0,
  };
}

// Add one event's counters to a running sum. Each event's cacheWrite1h is
// clamped to its own cacheWrite first, so one malformed event cannot rebill
// other events' writes at the 1h rate. Mirrors add_tokens in rollup.rs.
function addTokens(sum: TokenCounts, event: TokenCounts): void {
  sum.input += event.input;
  sum.output += event.output;
  sum.cacheRead += event.cacheRead;
  sum.cacheWrite += event.cacheWrite;
  sum.reasoning += event.reasoning;
  if (event.cacheWrite1h !== undefined) {
    const reported1h = Number.isFinite(event.cacheWrite1h) ? event.cacheWrite1h : 0;
    const clamped1h = Math.min(Math.max(reported1h, 0), Math.max(event.cacheWrite, 0));
    sum.cacheWrite1h = (sum.cacheWrite1h ?? 0) + clamped1h;
  }
}

function accumulate(entry: Rollup, event: UsageEvent): void {
  addTokens(entry.tokens, event.tokens);
  entry.events += 1;
  if (event.turn) entry.turns += 1;
  const reported = event.calls;
  entry.calls += reported !== undefined && Number.isFinite(reported) ? Math.max(0, reported) : 1;
  const recorded = event.costUsd;
  if (recorded !== undefined && Number.isFinite(recorded)) {
    entry.costUsd = (entry.costUsd ?? 0) + recorded;
  }
}

function daysFromCivil(year: number, month: number, day: number): number {
  const y = month <= 2 ? year - 1 : year;
  const era = Math.floor((y >= 0 ? y : y - 399) / 400);
  const yoe = y - era * 400;
  const m = month;
  const doy = Math.floor((153 * (m > 2 ? m - 3 : m + 9) + 2) / 5) + day - 1;
  const doe = yoe * 365 + Math.floor(yoe / 4) - Math.floor(yoe / 100) + doy;
  return era * 146_097 + doe - 719_468;
}

// Parse an ECMA-262 Date Time String Format timestamp to epoch millis:
// YYYY-MM-DD (T HH:mm(:ss(.sss)?)?)? (Z | ±HH:mm)?, offsetless date-time is UTC.
// Anything else is undefined, so both twins bucket the same non-canonical input
// into the invalid-date block rather than letting host Date leniency diverge.
// Mirrors date_parse_ms in time.rs.
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
        milli = Number(input.slice(fracStart, fracEnd).padEnd(3, "0").slice(0, 3));
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

// Milliseconds from the start of the wall-clock hour in tz to ms. Explicit
// "UTC" is direct arithmetic; an omitted tz is the system zone (the same path
// dayKey takes) and an unknown zone falls back to UTC. Subtracting it from the
// instant keeps the occurrence of a repeated (fall-back) hour. Mirrors
// within_hour in rollup.rs.
function withinHour(ms: number, tz: string | undefined): number {
  const subSecond = ((ms % 1000) + 1000) % 1000;
  if (tz === "UTC") return ((ms % HOUR_MS) + HOUR_MS) % HOUR_MS;
  try {
    const format = new Intl.DateTimeFormat("en-US", {
      ...(tz !== undefined ? { timeZone: tz } : {}),
      hour12: false,
      minute: "2-digit",
      second: "2-digit",
    });
    const parts: Record<string, string> = {};
    for (const p of format.formatToParts(new Date(ms))) parts[p.type] = p.value;
    const minute = Number(parts.minute === "24" ? "0" : parts.minute);
    return minute * 60_000 + Number(parts.second) * 1000 + subSecond;
  } catch {
    return ((ms % HOUR_MS) + HOUR_MS) % HOUR_MS;
  }
}

// The start of the block an event at ms opens: the instant floored to the width
// grain, measured from the start of its wall-clock hour. The grain is the width
// below one hour and the hour otherwise, so consecutive blocks always have
// distinct, ascending starts. Mirrors block_start in rollup.rs.
function blockStart(ms: number, withinHourMs: number, width: number): number {
  return ms - (withinHourMs % Math.min(width, HOUR_MS));
}

// The first instant past a block. Mirrors block_end in rollup.rs.
function blockEnd(start: number, width: number): number {
  return start + width;
}

// Session-window billing blocks: events ordered by instant, a block opening at
// the first event's block start in tz and holding events in [start, end).
// Unparseable timestamps sort last and share one trailing "invalid-date" row.
// The strict dateParseMs gives the boundary the same grammar as the Rust core.
// Mirrors block_rollup in rollup.rs.
function blockRollup(events: UsageEvent[], options: RollupOptions): Rollup[] {
  const width =
    options.blockMs !== undefined && Number.isFinite(options.blockMs) && options.blockMs > 0
      ? options.blockMs
      : DEFAULT_BLOCK_MS;

  const ordered = events.map((event) => ({ event, ms: dateParseMs(event.timestamp) }));
  ordered.sort((a, b) => {
    if (a.ms === undefined) return b.ms === undefined ? 0 : 1;
    if (b.ms === undefined) return -1;
    return a.ms - b.ms;
  });

  const out: Rollup[] = [];
  let current: Rollup | undefined;
  let end: number | undefined;
  for (const { event, ms } of ordered) {
    if (ms === undefined) {
      if (current?.key !== "invalid-date") current = emptyRollup("invalid-date");
    } else if (current === undefined || end === undefined || ms >= end) {
      const start = blockStart(ms, withinHour(ms, options.tz), width);
      end = blockEnd(start, width);
      current = emptyRollup(new Date(start).toISOString());
    }
    if (out.at(-1) !== current) out.push(current);
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
      entry = emptyRollup(key);
      map.set(key, entry);
    }
    accumulate(entry, event);
  }
  return [...map.values()].sort((a, b) => (a.key < b.key ? -1 : a.key > b.key ? 1 : 0));
}
