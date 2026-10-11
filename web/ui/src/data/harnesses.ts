export type HarnessField = "costUsd" | "workspace" | "title" | "calls" | "subagent";

export type HarnessEntry = {
  id: string;
  name: string;
  fields: HarnessField[];
  estimated?: true;
};

export const harnesses: HarnessEntry[] = [
  { id: "claude", name: "Claude Code", fields: ["costUsd", "subagent"] },
  { id: "codex", name: "Codex CLI", fields: [] },
  { id: "gemini", name: "Gemini CLI", fields: [] },
  { id: "opencode", name: "OpenCode", fields: ["costUsd", "subagent", "title", "workspace"] },
  { id: "mimocode", name: "MiMo Code", fields: ["costUsd", "subagent", "title", "workspace"] },
  {
    id: "commandcode",
    name: "Command Code",
    fields: ["costUsd", "subagent", "title", "workspace"],
  },
  { id: "copilot", name: "GitHub Copilot CLI", fields: [] },
  { id: "amp", name: "Amp CLI", fields: [] },
  { id: "droid", name: "Droid CLI", fields: [] },
  { id: "qwen", name: "Qwen Code", fields: [] },
  { id: "pi", name: "Pi", fields: ["subagent"] },
  { id: "omp", name: "omp", fields: [] },
  { id: "prime", name: "Prime Agent", fields: ["subagent"] },
  { id: "gajae", name: "Gajae-Code", fields: [] },
  { id: "kimchi", name: "Kimchi Coding", fields: [] },
  { id: "grok", name: "Grok Build", fields: [] },
  { id: "augment", name: "Auggie CLI", fields: [] },
  { id: "codebuff", name: "Codebuff", fields: [] },
  { id: "codebuddy", name: "CodeBuddy Code", fields: ["workspace"] },
  { id: "jcode", name: "jcode", fields: [] },
  { id: "mux", name: "Xum", fields: [] },
  { id: "zcode", name: "ZCode", fields: [] },
  { id: "roo", name: "Roo Code", fields: [] },
  { id: "cline", name: "Cline", fields: [] },
  { id: "kilo", name: "Kilo Code CLI", fields: [] },
  { id: "kilocode", name: "Kilo Code for VS Code", fields: [] },
  { id: "openclaw", name: "OpenClaw", fields: [] },
  { id: "kimi", name: "Kimi Code CLI", fields: [] },
  { id: "junie", name: "Junie", fields: [] },
  { id: "devin", name: "Devin CLI", fields: [] },
  { id: "hermes", name: "Hermes Agent", fields: ["calls"] },
  { id: "goose", name: "goose", fields: [] },
  { id: "zed", name: "Zed Agent", fields: ["calls"] },
  { id: "cherrystudio", name: "Cherry Studio", fields: [] },
  { id: "opencodereview", name: "OpenCodeReview", fields: [] },
  { id: "trae", name: "Trae Agent", fields: [] },
  { id: "deepseek", name: "DeepSeek Harness", fields: ["subagent", "workspace"] },
  { id: "reasonix", name: "Reasonix", fields: ["title", "workspace"] },
  { id: "kiro", name: "Kiro CLI", fields: ["workspace"], estimated: true },
  { id: "fx", name: "fx", fields: ["calls", "costUsd"] },
];

export const unsupported = ["Cursor", "aider", "Crush", "GitLab Duo CLI"];

export const harnessName = (id: string): string => harnesses.find((h) => h.id === id)?.name ?? id;
