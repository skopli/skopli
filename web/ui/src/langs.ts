export const langs = {
  ts: "TypeScript",
  python: "Python",
  rust: "Rust",
  ruby: "Ruby",
  java: "Java",
  csharp: "C#",
  go: "Go",
  swift: "Swift",
  c: "C",
} as const;

export type Lang = keyof typeof langs;
