import mdx from "@astrojs/mdx";
import { codeThemes } from "@skopli/ui/langs";
import sitemap from "@astrojs/sitemap";
import { defineConfig } from "astro/config";
import { writeFile } from "node:fs/promises";
import { fileURLToPath } from "node:url";

const rootRedirect = () => ({
  name: "root-redirect",
  hooks: {
    "astro:build:done": async ({ dir }) => {
      const html =
        '<!doctype html><meta charset="utf-8"><meta http-equiv="refresh" content="0; url=/skopli/"><title>Skopli docs</title><a href="/skopli/">Skopli docs</a>';
      await writeFile(fileURLToPath(new URL("../index.html", dir)), html);
    },
  },
});

export default defineConfig({
  site: "https://docs.skopli.com",
  base: "/skopli/",
  outDir: "./dist/skopli",
  trailingSlash: "always",
  integrations: [mdx(), sitemap(), rootRedirect()],
  markdown: {
    shikiConfig: {
      themes: codeThemes,
      defaultColor: false,
    },
  },
});
