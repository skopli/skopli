export type Snippet = {
  label?: string;
  lang: "sh" | "xml" | "groovy" | "swift" | "c";
  code: string;
};
export type Language = { id: string; name: string; note?: "swift" | "c"; install: Snippet[] };

export const LANGUAGES: readonly Language[] = [
  {
    id: "typescript",
    name: "TypeScript",
    install: [
      { label: "npm", lang: "sh", code: "npm install skopli" },
      { label: "pnpm", lang: "sh", code: "pnpm add skopli" },
      { label: "bun", lang: "sh", code: "bun add skopli" },
    ],
  },
  { id: "python", name: "Python", install: [{ lang: "sh", code: "pip install skopli" }] },
  { id: "rust", name: "Rust", install: [{ lang: "sh", code: "cargo add skopli-core" }] },
  { id: "ruby", name: "Ruby", install: [{ lang: "sh", code: "gem install skopli" }] },
  {
    id: "java",
    name: "Java",
    install: [
      {
        label: "Maven",
        lang: "xml",
        code: "<dependency>\n  <groupId>com.skopli</groupId>\n  <artifactId>skopli</artifactId>\n</dependency>",
      },
      { label: "Gradle", lang: "groovy", code: 'implementation("com.skopli:skopli")' },
    ],
  },
  { id: "csharp", name: "C#", install: [{ lang: "sh", code: "dotnet add package Skopli" }] },
  {
    id: "go",
    name: "Go",
    install: [{ lang: "sh", code: "go get github.com/skopli/skopli/sdks/go" }],
  },
  {
    id: "swift",
    name: "Swift",
    note: "swift",
    install: [
      { lang: "swift", code: '.package(url: "https://github.com/skopli/skopli", from: "1.0.0")' },
    ],
  },
  {
    id: "c",
    name: "C/C++",
    note: "c",
    install: [
      { lang: "c", code: '#include "skopli.h"' },
      { lang: "sh", code: "cc app.c -lskopli" },
    ],
  },
];
