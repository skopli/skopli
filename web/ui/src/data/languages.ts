export type Language = { id: string; name: string };

export const LANGUAGES: readonly Language[] = [
  { id: "ts", name: "TypeScript" },
  { id: "py", name: "Python" },
  { id: "rs", name: "Rust" },
  { id: "rb", name: "Ruby" },
  { id: "java", name: "Java" },
  { id: "cs", name: "C#" },
  { id: "go", name: "Go" },
  { id: "swift", name: "Swift" },
  { id: "c", name: "C/C++" },
];

export const LANGUAGE_COUNT = LANGUAGES.length;
