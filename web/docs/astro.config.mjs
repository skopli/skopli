import mdx from "@astrojs/mdx";
import { codeThemes } from "@skopli/ui/langs";
import { project } from "@skopli/ui/project";
import sitemap from "@astrojs/sitemap";
import { defineConfig } from "astro/config";
import { writeFile } from "node:fs/promises";
import { fileURLToPath } from "node:url";
import * as pagefind from "pagefind";
import { BASE, SITE } from "./src/nav.ts";

const base = `${BASE}/`;

const failOn = (errors) => {
  if (errors.length) throw new Error(errors.join("\n"));
};

const hostRoot = () => ({
  name: "host-root",
  hooks: {
    "astro:build:done": async ({ dir }) => {
      const host = new URL("../", dir);
      const files = {
        "index.html": `<!doctype html><meta charset="utf-8"><meta http-equiv="refresh" content="0; url=${base}"><title>${project.docsName}</title><a href="${base}">${project.docsName}</a>`,
        "robots.txt": `User-agent: *\nAllow: /\n\nSitemap: ${SITE}${base}sitemap-index.xml\n`,
      };
      for (const [name, body] of Object.entries(files)) {
        await writeFile(new URL(name, host), body);
      }
      const { errors, index } = await pagefind.createIndex();
      failOn(errors);
      failOn((await index.addDirectory({ path: fileURLToPath(host) })).errors);
      failOn(
        (await index.writeFiles({ outputPath: fileURLToPath(new URL("pagefind/", dir)) })).errors,
      );
      await pagefind.close();
    },
  },
});

export default defineConfig({
  site: SITE,
  base,
  outDir: `./dist${BASE}`,
  trailingSlash: "always",
  integrations: [mdx(), sitemap(), hostRoot()],
  markdown: {
    shikiConfig: {
      themes: codeThemes,
      defaultColor: false,
    },
  },
});
