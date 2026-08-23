export type Harness =
  | "claude"
  | "opencode"
  | "mimocode"
  | "commandcode"
  | "codex"
  | "gemini"
  | "copilot"
  | "amp"
  | "droid"
  | "qwen"
  | "pi"
  | "omp"
  | "prime"
  | "gajae"
  | "kimchi"
  | "kilo"
  | "goose"
  | "hermes"
  | "kimi"
  | "openclaw"
  | "junie"
  | "grok"
  | "augment"
  | "codebuff"
  | "codebuddy"
  | "jcode"
  | "mux"
  | "zed"
  | "cherrystudio"
  | "zcode"
  | "devin"
  | "roo"
  | "cline"
  | "kilocode"
  | "opencodereview"
  | "trae"
  | "deepseek"
  | "reasonix"
  | "kiro"
  | "fx";

export type TokenCounts = {
  input: number;
  output: number;
  cacheRead: number;
  // total cache-write tokens across all TTLs
  cacheWrite: number;
  // 1h-TTL portion of cacheWrite, when the source reports the split; absent
  // means all writes are treated as 5m
  cacheWrite1h?: number;
  reasoning: number;
};

export type UsageEvent = {
  harness: Harness;
  timestamp: string;
  sessionId: string;
  messageId: string;
  turn: boolean;
  subagent: boolean;
  model: string;
  tokens: TokenCounts;
  // number of underlying model calls this event aggregates, when the source
  // reports it (e.g. hermes message_count). Absent means a single call.
  calls?: number;
  costUsd?: number;
  workspace?: string;
  title?: string;
};

export type DiagnosticSeverity = "warning" | "info";

export type Diagnostic = {
  severity: DiagnosticSeverity;
  message: string;
  harness?: Harness;
  path?: string;
};
