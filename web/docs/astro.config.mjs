import { defineConfig } from "astro/config";
import mdx from "@astrojs/mdx";
import { codeTheme } from "@skopli/ui/code-theme";
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

/** @returns {import("astro").AstroIntegration} */
const rootRedirect = () => ({
  name: "root-redirect",
  hooks: {
    "astro:build:done": ({ dir }) =>
      writeFile(join(fileURLToPath(dir), "..", "index.html"), ROOT_REDIRECT),
  },
});

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
