import { writeFile } from "node:fs/promises";
import { defineConfig } from "astro/config";
import { codeThemes } from "@skopli/ui/code-theme.ts";
import { rehypeCodeTabs, tabMetaTransformer } from "@skopli/ui/code-tabs.ts";

const rootRedirect = () => ({
  name: "root-redirect",
  hooks: {
    "astro:build:done": async ({ dir }) => {
      await writeFile(
        new URL("../index.html", dir),
        '<!doctype html><meta charset="utf-8"><meta http-equiv="refresh" content="0; url=/skopli/"><link rel="canonical" href="https://docs.skopli.com/skopli/"><a href="/skopli/">Skopli docs</a>',
      );
    },
  },
});

export default defineConfig({
  site: "https://docs.skopli.com",
  base: "/skopli/",
  trailingSlash: "always",
  outDir: "./dist/skopli",
  build: { format: "directory" },
  integrations: [rootRedirect()],
  redirects: { "/contributing": "/reference/coverage/" },
  i18n: { defaultLocale: "en", locales: ["en"], routing: { prefixDefaultLocale: false } },
  markdown: {
    shikiConfig: { themes: codeThemes, defaultColor: false, transformers: [tabMetaTransformer] },
    rehypePlugins: [rehypeCodeTabs],
  },
});
