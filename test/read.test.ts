import { copyFileSync, mkdirSync, mkdtempSync, readdirSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { describe, expect, it } from "vitest";
import { readUsage } from "../src/read.ts";
import { rollup } from "../src/rollup.ts";

const goldenTraeInput = join(
  dirname(fileURLToPath(import.meta.url)),
  "..",
  "golden",
  "trae",
  "basic",
  "input",
  "trajectories",
);

const goldenCherryStudioHome = join(
  dirname(fileURLToPath(import.meta.url)),
  "..",
  "golden",
  "cherrystudio",
  "basic",
  "input",
);

const goldenDeepseekSqliteHome = join(
  dirname(fileURLToPath(import.meta.url)),
  "..",
  "golden",
  "deepseek",
  "sqlite",
  "input",
);

const goldenKiroHome = join(
  dirname(fileURLToPath(import.meta.url)),
  "..",
  "golden",
  "kiro",
  "basic",
  "input",
);

const goldenFxHome = join(
  dirname(fileURLToPath(import.meta.url)),
  "..",
  "golden",
  "fx",
  "basic",
  "input",
);

function makeClaudeHome(): string {
  const home = mkdtempSync(join(tmpdir(), "skopli-home-"));
  const project = join(home, ".claude", "projects", "demo");
  mkdirSync(project, { recursive: true });
  const line = (id: string, tokens: Record<string, number>): string =>
    JSON.stringify({
      type: "assistant",
      timestamp: "2026-08-01T10:00:00Z",
      sessionId: "s1",
      uuid: id,
      message: { id, model: "claude-sonnet-4-5", usage: tokens },
    });
  writeFileSync(
    join(project, "session.jsonl"),
    [
      line("m1", { input_tokens: 10, output_tokens: 5 }),
      "not json",
      line("m2", { input_tokens: 20, output_tokens: 6 }),
    ].join("\n"),
  );
  return home;
}

function makeHomeWithTimestamps(timestamps: string[]): string {
  const home = mkdtempSync(join(tmpdir(), "skopli-tz-"));
  const project = join(home, ".claude", "projects", "demo");
  mkdirSync(project, { recursive: true });
  writeFileSync(
    join(project, "session.jsonl"),
    timestamps
      .map((timestamp, index) =>
        JSON.stringify({
          type: "assistant",
          timestamp,
          sessionId: "s1",
          uuid: `m${index}`,
          message: {
            id: `m${index}`,
            model: "claude-sonnet-4-5",
            usage: { input_tokens: 10, output_tokens: 5 },
          },
        }),
      )
      .join("\n"),
  );
  return home;
}

describe("readUsage", () => {
  it("reads events for explicitly requested harnesses under an injected home", async () => {
    const home = makeClaudeHome();
    try {
      const result = await readUsage({ home, env: {}, harnesses: ["claude"] });
      expect(result.events).toHaveLength(2);
      expect(result.events[0]?.harness).toBe("claude");
      expect(result.diagnostics.some((d) => d.message.includes("malformed"))).toBe(true);
      expect(result.diagnostics[0]?.harness).toBe("claude");
    } finally {
      rmSync(home, { recursive: true, force: true });
    }
  });

  it("applies since/until filtering", async () => {
    const home = makeClaudeHome();
    try {
      const none = await readUsage({
        home,
        env: {},
        harnesses: ["claude"],
        until: "2026-07-01T00:00:00Z",
      });
      expect(none.events).toHaveLength(0);
      const all = await readUsage({
        home,
        env: {},
        harnesses: ["claude"],
        since: "2026-08-01T00:00:00Z",
      });
      expect(all.events).toHaveLength(2);
    } finally {
      rmSync(home, { recursive: true, force: true });
    }
  });

  it("includes subagents by default and excludes them on request", async () => {
    const home = mkdtempSync(join(tmpdir(), "skopli-subagent-"));
    try {
      const project = join(home, ".claude", "projects", "demo");
      mkdirSync(project, { recursive: true });
      const line = (id: string, sidechain: boolean): string =>
        JSON.stringify({
          type: "assistant",
          timestamp: "2026-08-01T10:00:00Z",
          sessionId: "s1",
          uuid: id,
          ...(sidechain ? { isSidechain: true } : {}),
          message: {
            id,
            model: "claude-sonnet-4-5",
            usage: { input_tokens: 10, output_tokens: 5 },
          },
        });
      writeFileSync(
        join(project, "session.jsonl"),
        [line("m1", false), line("m2", true)].join("\n"),
      );
      const included = await readUsage({ home, env: {}, harnesses: ["claude"] });
      expect(included.events.map((event) => event.messageId).sort()).toEqual(["m1", "m2"]);
      const excluded = await readUsage({
        home,
        env: {},
        harnesses: ["claude"],
        subagents: "exclude",
      });
      expect(excluded.events.map((event) => event.messageId)).toEqual(["m1"]);
    } finally {
      rmSync(home, { recursive: true, force: true });
    }
  });

  it("drops events with unusable timestamps and records a diagnostic even unfiltered", async () => {
    const home = mkdtempSync(join(tmpdir(), "skopli-badts-"));
    try {
      const project = join(home, ".claude", "projects", "demo");
      mkdirSync(project, { recursive: true });
      writeFileSync(
        join(project, "session.jsonl"),
        JSON.stringify({
          type: "assistant",
          timestamp: "not-a-timestamp",
          sessionId: "s1",
          uuid: "m1",
          message: { id: "m1", model: "claude-sonnet-4-5", usage: { input_tokens: 10 } },
        }),
      );
      const result = await readUsage({ home, env: {}, harnesses: ["claude"] });
      // the claude reader itself already rejects this record before the
      // readUsage backstop; the backstop is exercised by the amp test below
      expect(result.events).toHaveLength(0);
      expect(result.diagnostics.length).toBeGreaterThan(0);
    } finally {
      rmSync(home, { recursive: true, force: true });
    }
  });

  it("backstops reader-passed unusable timestamps with a diagnostic even unfiltered", async () => {
    // the amp reader passes ledger timestamp strings through unparsed, so a
    // garbage value reaches the readUsage-level policy
    const dir = mkdtempSync(join(tmpdir(), "skopli-amp-"));
    try {
      mkdirSync(join(dir, "threads"), { recursive: true });
      writeFileSync(
        join(dir, "threads", "T-1.json"),
        JSON.stringify({
          id: "T-1",
          usageLedger: {
            events: [
              {
                id: "bad",
                timestamp: "not-a-timestamp",
                model: "gpt-5",
                tokens: { input: 3, output: 4 },
              },
            ],
          },
        }),
      );
      const result = await readUsage({ env: { AMP_DATA_DIR: dir }, harnesses: ["amp"] });
      expect(result.events).toHaveLength(0);
      const backstop = result.diagnostics.find((d) => d.message.includes("unusable timestamp"));
      expect(backstop).toBeDefined();
      expect(backstop?.harness).toBe("amp");
    } finally {
      rmSync(dir, { recursive: true, force: true });
    }
  });

  it("rejects invalid dates", async () => {
    await expect(readUsage({ harnesses: [], since: "not-a-date" })).rejects.toThrow(RangeError);
    await expect(readUsage({ harnesses: [], since: "2026-02-30" })).rejects.toThrow(RangeError);
  });

  it("resolves date-only years below 0100 without the legacy Date.UTC offset", async () => {
    const home = makeHomeWithTimestamps(["2026-08-01T10:00:00Z"]);
    try {
      const after = await readUsage({
        home,
        env: {},
        harnesses: ["claude"],
        since: "0099-01-01",
        tz: "UTC",
      });
      expect(after.events).toHaveLength(1);
      const before = await readUsage({
        home,
        env: {},
        harnesses: ["claude"],
        until: "0099-01-01",
        tz: "UTC",
      });
      expect(before.events).toHaveLength(0);
      const yearZero = await readUsage({
        home,
        env: {},
        harnesses: ["claude"],
        since: "0000-01-01",
        tz: "UTC",
      });
      expect(yearZero.events).toHaveLength(1);
    } finally {
      rmSync(home, { recursive: true, force: true });
    }
  });

  it("interprets date-only since in the given timezone", async () => {
    const home = makeHomeWithTimestamps(["2026-08-01T00:30:00Z", "2026-08-01T10:00:00Z"]);
    try {
      const filtered = await readUsage({
        home,
        env: {},
        harnesses: ["claude"],
        since: "2026-08-01",
        tz: "America/Los_Angeles",
      });
      expect(filtered.events).toHaveLength(1);
      expect(filtered.events[0]?.timestamp).toBe("2026-08-01T10:00:00Z");
      const byDay = rollup(filtered.events, { by: "day", tz: "America/Los_Angeles" });
      expect(byDay.map((r) => r.key)).toEqual(["2026-08-01"]);
    } finally {
      rmSync(home, { recursive: true, force: true });
    }
  });

  it("keeps full ISO since semantics unchanged regardless of tz", async () => {
    const home = makeHomeWithTimestamps(["2026-08-01T00:30:00Z", "2026-08-01T10:00:00Z"]);
    try {
      const result = await readUsage({
        home,
        env: {},
        harnesses: ["claude"],
        since: "2026-08-01T00:00:00Z",
        tz: "America/Los_Angeles",
      });
      expect(result.events).toHaveLength(2);
    } finally {
      rmSync(home, { recursive: true, force: true });
    }
  });

  it("resolves date-only bounds in the system timezone when tz is omitted", async () => {
    // TZ env manipulation is unreliable on Windows, so compare the default
    // path against an explicit call with the resolved system zone; the
    // fixtures straddle the system-zone midnight so a wrong UTC fallback
    // would select a different set whenever the system zone is not UTC
    const systemTz = Intl.DateTimeFormat().resolvedOptions().timeZone;
    const dayStart = Date.parse("2026-08-01T00:00:00Z");
    const offsetMs =
      Date.parse(
        `${new Intl.DateTimeFormat("sv-SE", {
          timeZone: systemTz,
          year: "numeric",
          month: "2-digit",
          day: "2-digit",
          hour: "2-digit",
          minute: "2-digit",
          second: "2-digit",
          hourCycle: "h23",
        })
          .format(dayStart)
          .replace(" ", "T")}Z`,
      ) - dayStart;
    const iso = (ms: number): string => new Date(ms).toISOString();
    const home = makeHomeWithTimestamps([
      iso(dayStart - offsetMs + 60_000),
      iso(dayStart - offsetMs + 86_400_000 - 60_000),
    ]);
    try {
      const defaulted = await readUsage({
        home,
        env: {},
        harnesses: ["claude"],
        since: "2026-08-01",
        until: "2026-08-01",
      });
      const explicit = await readUsage({
        home,
        env: {},
        harnesses: ["claude"],
        since: "2026-08-01",
        until: "2026-08-01",
        tz: systemTz,
      });
      const utc = await readUsage({
        home,
        env: {},
        harnesses: ["claude"],
        since: "2026-08-01",
        until: "2026-08-01",
        tz: "UTC",
      });
      const stamps = (r: Awaited<ReturnType<typeof readUsage>>): string[] =>
        r.events.map((e) => e.timestamp);
      expect(stamps(defaulted)).toEqual(stamps(explicit));
      expect(stamps(explicit)).toEqual([
        iso(dayStart - offsetMs + 60_000),
        iso(dayStart - offsetMs + 86_400_000 - 60_000),
      ]);
      if (offsetMs !== 0) expect(stamps(utc)).not.toEqual(stamps(explicit));
    } finally {
      rmSync(home, { recursive: true, force: true });
    }
  });

  it("treats date-only until as the exclusive end of that day in the given timezone", async () => {
    const home = makeHomeWithTimestamps(["2026-08-02T06:30:00Z", "2026-08-02T07:30:00Z"]);
    try {
      const result = await readUsage({
        home,
        env: {},
        harnesses: ["claude"],
        until: "2026-08-01",
        tz: "America/Los_Angeles",
      });
      expect(result.events).toHaveLength(1);
      expect(result.events[0]?.timestamp).toBe("2026-08-02T06:30:00Z");
    } finally {
      rmSync(home, { recursive: true, force: true });
    }
  });

  it("resolves date-only bounds correctly across DST transitions", async () => {
    const home = makeHomeWithTimestamps([
      "2026-03-08T07:30:00Z",
      "2026-03-08T08:30:00Z",
      "2026-11-01T06:30:00Z",
      "2026-11-01T07:30:00Z",
    ]);
    try {
      const springForward = await readUsage({
        home,
        env: {},
        harnesses: ["claude"],
        since: "2026-03-08",
        until: "2026-03-09",
        tz: "America/Los_Angeles",
      });
      expect(springForward.events.map((e) => e.timestamp)).toEqual(["2026-03-08T08:30:00Z"]);
      const fallBack = await readUsage({
        home,
        env: {},
        harnesses: ["claude"],
        since: "2026-11-01",
        tz: "America/Los_Angeles",
      });
      expect(fallBack.events.map((e) => e.timestamp)).toEqual(["2026-11-01T07:30:00Z"]);
    } finally {
      rmSync(home, { recursive: true, force: true });
    }
  });

  it("uses the first valid instant when midnight falls in a DST gap", async () => {
    const home = makeHomeWithTimestamps(["2026-09-06T03:30:00Z", "2026-09-06T04:30:00Z"]);
    try {
      const sinceGap = await readUsage({
        home,
        env: {},
        harnesses: ["claude"],
        since: "2026-09-06",
        tz: "America/Santiago",
      });
      expect(sinceGap.events.map((e) => e.timestamp)).toEqual(["2026-09-06T04:30:00Z"]);
      const untilGap = await readUsage({
        home,
        env: {},
        harnesses: ["claude"],
        until: "2026-09-05",
        tz: "America/Santiago",
      });
      expect(untilGap.events.map((e) => e.timestamp)).toEqual(["2026-09-06T03:30:00Z"]);
    } finally {
      rmSync(home, { recursive: true, force: true });
    }
  });

  it("resolves an ambiguous midnight (DST overlap) to its first occurrence", async () => {
    const home = makeHomeWithTimestamps(["2020-10-29T21:30:00Z", "2020-10-29T20:30:00Z"]);
    try {
      const sinceOverlap = await readUsage({
        home,
        env: {},
        harnesses: ["claude"],
        since: "2020-10-30",
        tz: "Asia/Amman",
      });
      expect(sinceOverlap.events.map((e) => e.timestamp)).toEqual(["2020-10-29T21:30:00Z"]);
      const untilOverlap = await readUsage({
        home,
        env: {},
        harnesses: ["claude"],
        until: "2020-10-29",
        tz: "Asia/Amman",
      });
      expect(untilOverlap.events.map((e) => e.timestamp)).toEqual(["2020-10-29T20:30:00Z"]);
    } finally {
      rmSync(home, { recursive: true, force: true });
    }
  });

  it("reads the trae golden through an injected cwd that differs from process.cwd()", async () => {
    // trae discovers trajectories under <cwd>/trajectories, so the facade must
    // forward the injected cwd to the native reader rather than falling back to
    // the Node process working directory
    const project = mkdtempSync(join(tmpdir(), "skopli-trae-cwd-"));
    try {
      expect(project).not.toBe(process.cwd());
      const trajectories = join(project, "trajectories");
      mkdirSync(trajectories, { recursive: true });
      for (const name of readdirSync(goldenTraeInput)) {
        copyFileSync(join(goldenTraeInput, name), join(trajectories, name));
      }
      const result = await readUsage({ harnesses: ["trae"], cwd: project });
      expect(result.events).toHaveLength(4);
      expect(result.events.every((event) => event.harness === "trae")).toBe(true);
      const first = result.events[0];
      expect(first?.model).toBe("trae-sonnet");
      expect(first?.tokens.cacheRead).toBe(300);
      expect(first?.tokens.cacheWrite).toBe(200);
      expect(first?.tokens.reasoning).toBe(40);
    } finally {
      rmSync(project, { recursive: true, force: true });
    }
  });

  it("reads the cherrystudio golden SQLite database through an injected home", async () => {
    // Cherry Studio reads the single cherrystudio.sqlite under
    // <home>/.config/CherryStudio/Data; the golden home wires that path so the
    // facade drives the native SQLite reader end to end.
    const result = await readUsage({
      home: goldenCherryStudioHome,
      env: {},
      harnesses: ["cherrystudio"],
      tz: "UTC",
    });
    expect(result.events).toHaveLength(2);
    expect(result.events.every((event) => event.harness === "cherrystudio")).toBe(true);
    const bySession = new Map(result.events.map((event) => [event.sessionId, event]));
    const inv = bySession.get("inv1");
    expect(inv?.model).toBe("anthropic/claude-sonnet-4-5");
    expect(inv?.tokens.cacheRead).toBe(20);
    expect(inv?.tokens.cacheWrite).toBe(8);
    expect(inv?.tokens.reasoning).toBe(10);
    expect(inv?.timestamp).toBe("2025-08-01T10:00:00.000Z");
    const agg = bySession.get("agg1");
    expect(agg?.model).toBe("openai/gpt-4o");
    expect(agg?.tokens.input).toBe(1000);
  });

  it("reads the deepseek golden SQLite backend and dedupes against JSONL through the facade", async () => {
    // DeepSeek Harness can persist sessions to SQLite instead of JSONL; the
    // golden pairs both backends with one shared session_id:seq so the facade
    // drives the native reader's dedupe (JSONL wins) end to end.
    const result = await readUsage({
      home: "/nonexistent",
      env: { DSH_HOME: goldenDeepseekSqliteHome },
      harnesses: ["deepseek"],
      tz: "UTC",
    });
    expect(result.events).toHaveLength(4);
    expect(result.events.every((event) => event.harness === "deepseek")).toBe(true);
    const jsonlWinner = result.events.find(
      (event) => event.sessionId === "sess-main" && event.tokens.input === 100,
    );
    expect(jsonlWinner?.messageId.endsWith("session.jsonl:1")).toBe(true);
    expect(jsonlWinner?.tokens.cacheRead).toBe(30);
    const sqliteOnly = result.events.find((event) => event.messageId === "sess-db:1");
    expect(sqliteOnly?.model).toBe("deepseek-reasoner");
    expect(sqliteOnly?.subagent).toBe(true);
    expect(sqliteOnly?.tokens.reasoning).toBe(30);
    const zstdBlob = result.events.find((event) => event.messageId === "sess-db:2");
    expect(zstdBlob?.model).toBe("deepseek-reasoner");
    expect(zstdBlob?.tokens.input).toBe(40);
    expect(zstdBlob?.tokens.output).toBe(15);
  });

  it("reads the kiro golden SQLite database with estimated tokens through an injected home", async () => {
    // Kiro CLI reads ~/.kiro/data.sqlite3; the golden home wires that path so
    // the facade drives the native reader end to end. Kiro persists no provider
    // token counts, so counts are estimated from the persisted text: the UTF-8
    // byte length divided by four, rounded to the nearest ten.
    const result = await readUsage({
      home: goldenKiroHome,
      env: {},
      harnesses: ["kiro"],
      tz: "UTC",
    });
    expect(result.events).toHaveLength(3);
    expect(result.events.every((event) => event.harness === "kiro")).toBe(true);
    const first = result.events.find((event) => event.messageId === "/home/u/proj:0");
    expect(first?.model).toBe("claude-sonnet-4-5");
    expect(first?.sessionId).toBe("/home/u/proj");
    expect(first?.turn).toBe(true);
    expect(first?.tokens.input).toBe(10);
    expect(first?.tokens.output).toBe(20);
    expect(first?.timestamp).toBe("2026-08-01T10:00:00.000Z");
    const second = result.events.find((event) => event.messageId === "/home/u/proj:1");
    expect(second?.tokens.input).toBe(20);
    expect(second?.tokens.output).toBe(30);
    const other = result.events.find((event) => event.messageId === "/home/u/other:0");
    expect(other?.model).toBe("amazon-q-default");
    expect(other?.tokens.input).toBe(10);
    expect(other?.tokens.output).toBe(10);
    expect(result.skipped.kiro?.length).toBe(1);
    expect(result.skipped.kiro?.[0]).toContain(":conversations:1");
  });

  it("reads the fx golden usage-v2 sidecars with provider token counts through an injected home", async () => {
    // fx persists per-session usage at ~/.fx/sessions/<id>/usage-v2.json; the
    // golden home wires that path so the facade drives the native reader end to
    // end. fx records real provider token counts, and each ModelAggregate in a
    // snapshot becomes one event regardless of billing state.
    const result = await readUsage({
      home: goldenFxHome,
      env: {},
      harnesses: ["fx"],
      tz: "UTC",
    });
    expect(result.events).toHaveLength(4);
    expect(result.events.every((event) => event.harness === "fx")).toBe(true);
    const first = result.events.find(
      (event) =>
        event.messageId === "1770000000000-1770000000000000000-a1b2c3d4e5f60718:openai/gpt-5.4",
    );
    expect(first?.model).toBe("openai/gpt-5.4");
    expect(first?.tokens.input).toBe(20);
    expect(first?.tokens.output).toBe(8);
    expect(first?.tokens.reasoning).toBe(4);
    expect(first?.costUsd).toBe(0.03);
    expect(first?.timestamp).toBe("2026-02-02T02:40:00.000Z");
    const legacy = result.events.find(
      (event) =>
        event.messageId === "1770000100000-1770000000000000001-b1b2c3d4e5f60718:openai/gpt-5.4",
    );
    expect(legacy?.tokens.reasoning).toBe(0);
    expect(legacy?.calls).toBeUndefined();
    expect(result.skipped.fx?.length).toBe(1);
    expect(result.skipped.fx?.[0]).toContain(":usage-v2:4");
  });

  it("auto-detects harnesses in an empty injected home without touching the real one", async () => {
    const home = mkdtempSync(join(tmpdir(), "skopli-empty-"));
    try {
      const result = await readUsage({ home, env: {} });
      expect(result.events).toEqual([]);
      expect(result.diagnostics).toEqual([]);
    } finally {
      rmSync(home, { recursive: true, force: true });
    }
  });
});
