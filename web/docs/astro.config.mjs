import mdx from "@astrojs/mdx";
import { codeThemes } from "@skopli/ui/langs";
import sitemap from "@astrojs/sitemap";
import { defineConfig } from "astro/config";
import { writeFile } from "node:fs/promises";
import { fileURLToPath } from "node:url";
import { BASE, SITE } from "./src/nav.ts";

const base = `${BASE}/`;

const hostRoot = () => ({
  name: "host-root",
  hooks: {
    "astro:build:done": async ({ dir }) => {
      const files = {
        "index.html": `<!doctype html><meta charset="utf-8"><meta http-equiv="refresh" content="0; url=${base}"><title>Skopli docs</title><a href="${base}">Skopli docs</a>`,
        "robots.txt": `User-agent: *\nAllow: /\n\nSitemap: ${SITE}${base}sitemap-index.xml\n`,
      };
      for (const [name, body] of Object.entries(files)) {
        await writeFile(fileURLToPath(new URL(`../${name}`, dir)), body);
      }
    },
  },
});

export default defineConfig({
  site: SITE,
  base,
  outDir: "./dist/skopli",
  trailingSlash: "always",
  integrations: [mdx(), sitemap(), hostRoot()],
  markdown: {
    shikiConfig: {
      themes: codeThemes,
      defaultColor: false,
    },
  },
});
