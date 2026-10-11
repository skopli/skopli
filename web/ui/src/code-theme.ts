import type { ThemeRegistrationRaw } from "shiki";

// Two Shiki themes drawn from the tokens: a pale ink on the sunken ground at night,
// dark ink on paper by day. Few colours: the code reads like a table, not a parade.
type Palette = {
  bg: string;
  fg: string;
  muted: string;
  accent: string;
  string: string;
  number: string;
  type: string;
};
const theme = (name: string, c: Palette): ThemeRegistrationRaw => ({
  name,
  type: name === "skopli-light" ? "light" : "dark",
  colors: { "editor.background": c.bg, "editor.foreground": c.fg },
  settings: [
    { settings: { background: c.bg, foreground: c.fg } },
    {
      scope: ["comment", "punctuation.definition.comment"],
      settings: { foreground: c.muted, fontStyle: "italic" },
    },
    {
      scope: ["keyword", "storage", "storage.type", "keyword.operator.new"],
      settings: { foreground: c.accent },
    },
    {
      scope: ["string", "string.quoted", "constant.character"],
      settings: { foreground: c.string },
    },
    { scope: ["constant.numeric", "constant.language"], settings: { foreground: c.number } },
    {
      scope: ["entity.name.function", "support.function", "meta.function-call"],
      settings: { foreground: c.fg },
    },
    {
      scope: ["entity.name.type", "support.type", "support.class", "entity.name.class"],
      settings: { foreground: c.type },
    },
    {
      scope: ["variable.parameter", "variable.other.property", "meta.object-literal.key"],
      settings: { foreground: c.fg },
    },
    { scope: ["punctuation", "meta.brace"], settings: { foreground: c.muted } },
  ],
});

export const codeThemes = {
  dark: theme("skopli-dark", {
    bg: "#060b17",
    fg: "#e8edf7",
    muted: "#7f8aa8",
    accent: "#ff6cb8",
    string: "#9fd3b3",
    number: "#f2c98a",
    type: "#9fc0ff",
  }),
  light: theme("skopli-light", {
    bg: "#fbf8f0",
    fg: "#141d36",
    muted: "#5f6884",
    accent: "#b0176a",
    string: "#1d6b3d",
    number: "#8a5300",
    type: "#1f4fa8",
  }),
};
