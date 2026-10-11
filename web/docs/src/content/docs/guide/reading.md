---
title: Reading usage
description: Every readUsage option, every UsageEvent field, and the semantics that differ between harnesses - time filters, subagents, token counters, diagnostics and skipped files.
---

## Options

```ts
type ReadUsageOptions = {
  harnesses?: Harness[]; // default: every detected harness
  since?: Date | string; // inclusive lower bound
  until?: Date | string; // exclusive upper bound
  tz?: string; // IANA zone for date-only bounds; default: system zone
  subagents?: "include" | "exclude"; // default: include
  // plus the path overrides described under "Custom homes"
};
```

`since` and `until` accept a `Date`, an ISO timestamp, or a date such as `2026-08-01`.
A date-only bound is interpreted at midnight in `tz`, so `since: "2026-08-01", tz: "America/Los_Angeles"` starts at 07:00 UTC.
Filtering happens on each event's own timestamp, not on file modification time, so a long session that straddles midnight is split correctly.

## The event

```ts
type UsageEvent = {
  harness: Harness; // "claude", "codex", ...
  timestamp: string; // ISO 8601, UTC
  sessionId: string;
  messageId: string;
  turn: boolean; // assistant record directly after a user record
  subagent: boolean;
  model: string; // as the log names it
  tokens: TokenCounts;
  calls?: number; // underlying model calls when the record aggregates several
  costUsd?: number; // only when the log itself reports a cost
  workspace?: string;
  title?: string;
};

type TokenCounts = {
  input: number;
  output: number;
  cacheRead: number;
  cacheWrite: number;
  cacheWrite1h?: number; // portion of cacheWrite written with a 1-hour TTL
  reasoning: number;
};
```

### Tokens are disjoint

The five counters never overlap.
Where a harness reports reasoning tokens inside its output count, the reader subtracts them so `output` is completion text only.
`cacheWrite1h` is not a sixth bucket: it is the part of `cacheWrite` that was written with a one-hour TTL and bills at a different rate.
Summing `input + output + cacheRead + cacheWrite + reasoning` gives the total tokens the request touched.

### Turns

`turn` is true for an assistant record that directly follows a user record.
Tool-call rounds, retries and continuation chunks are events but not turns.
Use turns when you want "how many prompts did I send", and events when you want "how many model responses were logged".

### Calls

Most harnesses log one record per model response, so `calls` is absent and counts as 1.
A few (Hermes Agent, Zed Agent, fx) aggregate a session or a tool loop into one record and report how many calls it covered; the reader copies that number into `calls`.
Rollups sum it, which is why a rollup's `calls` can exceed its `events`.

### Subagents

`subagent` is true when the log marks a record as produced by a sub-agent or side chain.
Only some harnesses record this (Claude Code, OpenCode, MiMo Code, Command Code, Pi, Prime Agent, DeepSeek Harness in the current fixtures).
A false value from a harness that never sets the flag is "unknown", not "no sub-agents were used".
`subagents: "exclude"` drops flagged records; it cannot drop what a harness never labelled.

### Optional fields

`costUsd`, `workspace`, `title` and `calls` appear only when the source log carries them.
The [harness reference](/skopli/reference/harnesses/) shows which fields each harness's golden fixture contains.
Never treat a missing `costUsd` as zero; price it instead.

## Diagnostics and skipped files

```ts
type ReadUsageResult = {
  events: UsageEvent[];
  diagnostics: Diagnostic[]; // { severity, message, harness?, path? }
  skipped: Partial<Record<Harness, string[]>>; // files that could not be used
};
```

A malformed line, an unknown schema version or an unreadable file is reported, not thrown.
`diagnostics` carry a severity and, where known, the harness and path.
`skipped` lists whole files the reader gave up on, grouped by harness.
One bad line does not cost you a month of data, but surface diagnostics in whatever you build.

## Estimated counts

Kiro CLI does not log token counts.
Its reader estimates tokens from message text length and marks the harness as estimated in the reference table.
Treat Kiro totals as approximate.

## Custom homes

Each reader looks in the harness's default location under your home directory.
Override the home when logs live elsewhere (a synced folder, a container volume, a test fixture).
To relocate one harness, set the environment variable its reader honours in `env`.
An explicit `env` replaces the process environment instead of merging with it, so include every variable you still need.

```ts tab=paths
const { events } = await readUsage({ home: "/mnt/backup/home/me" });
const synced = await readUsage({
  env: { ...process.env, CLAUDE_CONFIG_DIR: "/mnt/backup/claude" },
});
```

```python tab=paths
result = skopli.read_usage(home="/mnt/backup/home/me")
synced = skopli.read_usage(env={**os.environ, "CLAUDE_CONFIG_DIR": "/mnt/backup/claude"})
```

```ruby tab=paths
result = Skopli.read_usage(home: "/mnt/backup/home/me")
synced = Skopli.read_usage(env: ENV.to_h.merge("CLAUDE_CONFIG_DIR" => "/mnt/backup/claude"))
```

```go tab=paths
result, err := skopli.ReadUsage(skopli.ReadUsageOptions{Options: skopli.Options{Home: "/mnt/backup/home/me"}})
```

```java tab=paths
var result = Skopli.readUsage(ReadUsageOptions.builder().home("/mnt/backup/home/me").build());
```

```csharp tab=paths
var result = SkopliClient.ReadUsage(new ReadUsageOptions { Home = "/mnt/backup/home/me" });
```

```swift tab=paths
let result = try Skopli.readUsage(options: ReadUsageOptions(home: "/mnt/backup/home/me"))
```

```c tab=paths
const char *opts = "{\"home\":\"/mnt/backup/home/me\"}";
AgBuf result;
AgStatus st = ag_read_usage(opts, strlen(opts), &result);
```

The same overrides apply to `detectHarnesses`.
