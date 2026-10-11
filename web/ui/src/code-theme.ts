import { readFileSync } from "node:fs";

export type Theme = Record<string, string>;

export const parseThemes = (css: string): Record<string, Theme> => {
  const themes: Record<string, Theme> = {};
  for (const block of css.split("}")) {
    const [selector, body] = block.split("{");
    if (!selector?.includes(":root") || body === undefined) continue;
    const name = selector.includes('[data-theme="dark"]') ? "dark" : "light";
    themes[name] = Object.fromEntries(
      body.split(";").flatMap((decl) => {
        const [key, value] = decl.split(":").map((s) => s.trim());
        return key?.startsWith("--") && /^#[0-9a-fA-F]{6}$/.test(value ?? "")
          ? [[key.slice(2), value as string]]
          : [];
      }),
    );
  }
  return themes;
};

export const readTokens = (): Record<string, Theme> =>
  parseThemes(readFileSync(new URL("./styles/tokens.css", import.meta.url), "utf8"));

export const codeTheme = (type: "light" | "dark") => {
  const themes = readTokens();
  const c = { ...themes.light, ...(type === "dark" ? themes.dark : {}) };
  return {
    name: `ledger-${type}`,
    type,
    colors: { "editor.background": c["bg-raised"], "editor.foreground": c.fg },
    tokenColors: [
      {
        scope: ["comment", "punctuation.definition.comment"],
        settings: { foreground: c["fg-muted"], fontStyle: "italic" },
      },
      {
        scope: ["keyword", "storage", "keyword.control", "variable.language", "entity.name.tag"],
        settings: { foreground: c.accent },
      },
      {
        scope: [
          "string",
          "punctuation.definition.string",
          "constant.numeric",
          "constant.language",
          "constant.character",
        ],
        settings: { foreground: c.ok },
      },
    ],
  };
};
