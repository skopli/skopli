import type { HarnessIconSlug } from "./harness-icons.ts";

export interface Harness {
  id: string;
  name: string;
  icon?: HarnessIconSlug;
  note?: "reasonix-session" | "kiro-estimated" | "fx-aggregate";
}

export const harnesses: readonly Harness[] = [
  { id: "claude", name: "Claude Code", icon: "claude" },
  { id: "opencode", name: "OpenCode", icon: "opencode" },
  { id: "mimocode", name: "MiMo Code" },
  { id: "commandcode", name: "Command Code" },
  { id: "codex", name: "Codex CLI", icon: "codex" },
  { id: "gemini", name: "Gemini CLI", icon: "gemini" },
  { id: "copilot", name: "GitHub Copilot CLI", icon: "copilot" },
  { id: "amp", name: "Amp CLI", icon: "amp" },
  { id: "droid", name: "Droid CLI" },
  { id: "qwen", name: "Qwen Code", icon: "qwen" },
  { id: "pi", name: "Pi" },
  { id: "omp", name: "omp" },
  { id: "prime", name: "Prime Agent" },
  { id: "gajae", name: "Gajae-Code" },
  { id: "kimchi", name: "Kimchi Coding" },
  { id: "kilo", name: "Kilo Code CLI", icon: "kilocode" },
  { id: "goose", name: "goose", icon: "goose" },
  { id: "hermes", name: "Hermes Agent" },
  { id: "kimi", name: "Kimi Code CLI", icon: "kimi" },
  { id: "openclaw", name: "OpenClaw", icon: "openclaw" },
  { id: "junie", name: "Junie", icon: "junie" },
  { id: "grok", name: "Grok Build", icon: "grok" },
  { id: "augment", name: "Auggie CLI" },
  { id: "codebuff", name: "Codebuff" },
  { id: "codebuddy", name: "CodeBuddy Code" },
  { id: "jcode", name: "jcode" },
  { id: "mux", name: "Xum" },
  { id: "zed", name: "Zed Agent", icon: "zed" },
  { id: "cherrystudio", name: "Cherry Studio" },
  { id: "zcode", name: "ZCode", icon: "zcode" },
  { id: "devin", name: "Devin CLI", icon: "devin" },
  { id: "roo", name: "Roo Code", icon: "roocode" },
  { id: "cline", name: "Cline", icon: "cline" },
  { id: "kilocode", name: "Kilo Code for VS Code", icon: "kilocode" },
  { id: "opencodereview", name: "OpenCodeReview" },
  { id: "trae", name: "Trae Agent" },
  { id: "deepseek", name: "DeepSeek Harness" },
  { id: "reasonix", name: "Reasonix", note: "reasonix-session" },
  { id: "kiro", name: "Kiro CLI", note: "kiro-estimated" },
  { id: "fx", name: "fx", note: "fx-aggregate" },
];

export const unsupportedHarnesses = ["Cursor", "aider", "Crush", "GitLab Duo CLI"] as const;

export function harnessName(id: string): string {
  const harness = harnesses.find((h) => h.id === id);
  if (!harness) throw new Error(`unknown harness id: ${id}`);
  return harness.name;
}
