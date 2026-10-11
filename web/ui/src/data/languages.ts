export type Language = { id: string; name: string; install: string };

export const LANGUAGES: readonly Language[] = [
  { id: "ts", name: "TypeScript", install: "pnpm add skopli" },
  { id: "py", name: "Python", install: "pip install skopli" },
  { id: "rs", name: "Rust", install: "cargo add skopli" },
  { id: "rb", name: "Ruby", install: "gem install skopli" },
  { id: "java", name: "Java", install: "com.skopli:skopli" },
  { id: "cs", name: "C#", install: "dotnet add package Skopli" },
  { id: "go", name: "Go", install: "go get github.com/skopli/skopli-go" },
  { id: "swift", name: "Swift", install: "skopli-swift" },
  { id: "c", name: "C/C++", install: "libskopli" },
];

export const LANGUAGE_COUNT = LANGUAGES.length;
