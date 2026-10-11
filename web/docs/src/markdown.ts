const LANGS: Record<string, string> = {
  ts: "TypeScript",
  python: "Python",
  rust: "Rust",
  ruby: "Ruby",
  java: "Java",
  csharp: "C#",
  go: "Go",
  swift: "Swift",
  c: "C",
};

export const toMarkdown = (body: string): string =>
  body
    .replace(/^import \w+ from "\.[^"]*\.astro";\n/gm, "")
    .replace(/^<\/?Tabs>\n?/gm, "")
    .replace(/^<Fragment slot="(\w+)">\n/gm, (_, lang: string) => `### ${LANGS[lang] ?? lang}\n\n`)
    .replace(/^<\/Fragment>\n?/gm, "")
    .replace(/\n{3,}/g, "\n\n")
    .trim();

export const pageMarkdown = (
  title: string,
  description: string,
  body: string | undefined,
): string => `# ${title}\n\n> ${description}\n\n${toMarkdown(body ?? "")}\n`;
