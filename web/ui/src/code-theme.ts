import { readFileSync } from "node:fs";

export type Theme = Record<string, string>;

export const parseThemes = (css: string): Record<string, Theme> => {
  const themes: Record<string, Theme> = {};
  for (const block of css.matchAll(/:root(\[data-theme="(\w+)"\])?\s*\{([^}]*)\}/g)) {
    themes[block[2] ?? "light"] = Object.fromEntries(
      [...(block[3] ?? "").matchAll(/--([\w-]+):\s*(#[0-9a-fA-F]{6})/g)].map((m) => [m[1], m[2]]),
    ) as Theme;
  }
  return themes;
};

export const readTokens = (): Record<string, Theme> =>
  parseThemes(readFileSync(new URL("./styles/tokens.css", import.meta.url), "utf8"));

export const codeTheme = (type: "light" | "dark", css?: string) => {
  const themes = css ? parseThemes(css) : readTokens();
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
