import { fromWire, native } from "./native.ts";
import type { TokenCounts, UsageEvent } from "./types.ts";

export type RollupBy = "model" | "day" | "session" | "harness" | "workspace" | "block";

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

export function rollup(events: UsageEvent[], options: RollupOptions): Rollup[] {
  return fromWire<Rollup[]>(native().rollup(JSON.stringify(events), JSON.stringify(options)));
}
