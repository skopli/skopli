import { dateParseMs } from "./time.ts";
import type { TokenCounts, UsageEvent } from "./types.ts";

export type RollupBy = "model" | "day" | "session" | "harness" | "workspace" | "block";

// the default billing-block width: five hours in milliseconds
export const DEFAULT_BLOCK_MS = 18_000_000;

export type RollupOptions = {
  by: RollupBy;
  // IANA timezone for day bucketing and block-start flooring; defaults to
  // the system timezone
  tz?: string;
  // billing-block width in whole milliseconds (by === "block" only); a
  // missing, fractional or non-positive value uses DEFAULT_BLOCK_MS, the same
  // rule the wire applies when it reads blockMs as an i64
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

function emptyRollup(key: string): Rollup {
  return {
    key,
    tokens: { input: 0, output: 0, cacheRead: 0, cacheWrite: 0, reasoning: 0 },
    events: 0,
    turns: 0,
    calls: 0,
  };
}

// Add one event's counters to a running sum. Counters are JavaScript numbers,
// so a sum past Number.MAX_SAFE_INTEGER (2^53 - 1) keeps growing but loses
// integer precision, rounding to the nearest representable double, and a sum
// past Number.MAX_VALUE becomes Infinity; it never wraps. The Rust core
// saturates its u64 counters at 2^64 - 1 instead. Both rules keep a total
// monotone, and they agree everywhere below 2^53. Each event's cacheWrite1h is
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
      minute: "2-digit",
      second: "2-digit",
    });
    const parts: Record<string, string> = {};
    for (const p of format.formatToParts(new Date(ms))) parts[p.type] = p.value;
    return Number(parts.minute) * 60_000 + Number(parts.second) * 1000 + subSecond;
  } catch {
    return ((ms % HOUR_MS) + HOUR_MS) % HOUR_MS;
  }
}

// The start of the block an event at ms opens: the instant floored to the width
// grain, measured from the start of its wall-clock hour. The grain is the width
// below one hour and the hour otherwise, so consecutive blocks always have
// distinct, ascending starts. Because the grain restarts at each hour, a
// sub-hour width that does not divide the hour can start a block inside the
// previous block's [start, start + width); the earlier block keeps the events
// in that span. Mirrors block_start in rollup.rs.
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
    options.blockMs !== undefined && Number.isInteger(options.blockMs) && options.blockMs > 0
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
      if (current?.key !== "invalid-date") {
        current = emptyRollup("invalid-date");
        out.push(current);
      }
    } else if (current === undefined || end === undefined || ms >= end) {
      const start = blockStart(ms, withinHour(ms, options.tz), width);
      end = blockEnd(start, width);
      current = emptyRollup(new Date(start).toISOString());
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
      entry = emptyRollup(key);
      map.set(key, entry);
    }
    accumulate(entry, event);
  }
  return [...map.values()].sort((a, b) => (a.key < b.key ? -1 : a.key > b.key ? 1 : 0));
}
