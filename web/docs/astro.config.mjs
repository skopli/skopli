import { defineConfig } from "astro/config";
import { readFileSync } from "node:fs";
import { writeFile } from "node:fs/promises";
import { fileURLToPath } from "node:url";
import { join } from "node:path";

const ROOT_REDIRECT = `<!doctype html>
<html lang="en">
  <head>
    <meta charset="utf-8" />
    <meta http-equiv="refresh" content="0; url=/skopli/" />
    <link rel="canonical" href="https://docs.skopli.com/skopli/" />
    <title>Redirecting to Skopli docs</title>
  </head>
  <body>
    <p>Redirecting to <a href="/skopli/">/skopli/</a></p>
  </body>
</html>
`;

const tokensCss = readFileSync(new URL("../ui/src/styles/tokens.css", import.meta.url), "utf8");

/** Reads the colour tokens of one theme block in tokens.css. @param {string} selector */
const tokens = (selector) =>
  Object.fromEntries(
    [
      ...(
        tokensCss.match(
          new RegExp(`${selector.replace(/[[\]]/g, "\\$&")}\\s*\\{([^}]*)\\}`),
        )?.[1] ?? ""
      ).matchAll(/--([\w-]+):\s*(#[0-9a-fA-F]{6})/g),
    ].map((m) => [m[1], m[2]]),
  );

/** A Shiki theme built from the site tokens, so every code colour is already a tested AA pair. @param {"light" | "dark"} type */
const codeTheme = (type) => {
  const c = { ...tokens(":root"), ...(type === "dark" ? tokens(':root[data-theme="dark"]') : {}) };
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

/** Writes dist/index.html so the domain root redirects to the /skopli/ base. @returns {import("astro").AstroIntegration} */
const rootRedirect = () => ({
  name: "root-redirect",
  hooks: {
    "astro:build:done": ({ dir }) =>
      writeFile(join(fileURLToPath(dir), "..", "index.html"), ROOT_REDIRECT),
  },
});
import mdx from "@astrojs/mdx";

export default defineConfig({
  site: "https://docs.skopli.com",
  base: "/skopli/",
  outDir: "./dist/skopli",
  trailingSlash: "always",
  integrations: [mdx(), rootRedirect()],
  i18n: { defaultLocale: "en", locales: ["en"], routing: { prefixDefaultLocale: false } },
  vite: { build: { assetsInlineLimit: 0 } },
  markdown: {
    shikiConfig: {
      themes: { light: codeTheme("light"), dark: codeTheme("dark") },
      defaultColor: false,
    },
  },
});
