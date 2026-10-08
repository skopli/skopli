// @ts-check
import { mkdir, writeFile } from "node:fs/promises";
import { join } from "node:path";
import { fileURLToPath } from "node:url";
import { rehypeHeadingIds, unified } from "@astrojs/markdown-remark";
import mdx from "@astrojs/mdx";
import sitemap from "@astrojs/sitemap";
import { rehypeHeadingAnchors } from "@skopli/ui/markdown/rehype-heading-anchors.ts";
import { rehypeTableFrame } from "@skopli/ui/markdown/rehype-table-frame.ts";
import { remarkCallouts } from "@skopli/ui/markdown/remark-callouts.ts";
import { shikiFrame } from "@skopli/ui/markdown/shiki-frame.ts";
import { defaultLocale } from "@skopli/ui/i18n";
import { defineConfig } from "astro/config";
import * as pagefind from "pagefind";
import remarkDirective from "remark-directive";
import { locales, translatedSlugs } from "./src/lib/translated.ts";
import { basePath, siteOrigin } from "./src/site.ts";

const rootRedirect = `<!doctype html>
<html lang="en">
  <head>
    <meta charset="utf-8" />
    <meta http-equiv="refresh" content="0; url=${basePath}/" />
    <link rel="canonical" href="${siteOrigin}${basePath}/" />
    <title>Skopli docs</title>
  </head>
  <body>
    <p><a href="${basePath}/">Skopli docs</a></p>
  </body>
</html>
`;

/** @returns {import("astro").AstroIntegration} */
function postBuild() {
  return {
    name: "skopli-docs-post-build",
    hooks: {
      "astro:build:done": async ({ dir }) => {
        const out = fileURLToPath(dir);
        const domainRoot = join(out, "..");
        await mkdir(domainRoot, { recursive: true });
        await writeFile(join(domainRoot, "index.html"), rootRedirect);
        await writeFile(
          join(domainRoot, "robots.txt"),
          `User-agent: *\nAllow: /\n\nSitemap: ${siteOrigin}${basePath}/sitemap-index.xml\n`,
        );
        const { index, errors } = await pagefind.createIndex({
          rootSelector: "main",
          excludeSelectors: [".heading-anchor", ".code-frame__copy", ".callout__title"],
        });
        if (!index) throw new Error(errors.join("\n"));
        const added = await index.addDirectory({ path: out });
        if (added.errors.length) throw new Error(added.errors.join("\n"));
        const written = await index.writeFiles({ outputPath: join(out, "pagefind") });
        if (written.errors.length) throw new Error(written.errors.join("\n"));
        await pagefind.close();
      },
    },
  };
}

export default defineConfig({
  site: siteOrigin,
  base: basePath,
  outDir: `./dist${basePath}`,
  trailingSlash: "never",
  build: { format: "file" },
  i18n: {
    locales,
    defaultLocale,
    routing: { prefixDefaultLocale: false, redirectToDefaultLocale: false },
  },
  markdown: {
    processor: unified({
      remarkPlugins: [remarkDirective, remarkCallouts],
      rehypePlugins: [rehypeHeadingIds, rehypeTableFrame, rehypeHeadingAnchors],
    }),
    shikiConfig: { theme: "css-variables", transformers: [shikiFrame()] },
  },
  integrations: [
    mdx(),
    sitemap({
      filter: (page) => !page.endsWith("/404"),
      serialize: (item) => {
        const path = item.url.slice(`${siteOrigin}${basePath}`.length);
        const locale = locales.find(
          (l) => l !== defaultLocale && (path === `/${l}` || path.startsWith(`/${l}/`)),
        );
        const slug = locale ? path.slice(locale.length + 2) : path.replace(/^\//, "");
        const url = path === "" ? `${item.url}/` : item.url;
        const translated = locales.filter((l) => translatedSlugs[l]?.has(slug));
        if (translated.length === 0) return { ...item, url };
        /** @type {{ lang: string; url: string }[]} */
        const links = [defaultLocale, ...translated].map((l) => ({
          lang: l,
          url: `${siteOrigin}${basePath}${l === defaultLocale ? (slug ? `/${slug}` : "/") : `/${l}${slug ? `/${slug}` : ""}`}`,
        }));
        links.push({ lang: "x-default", url: links[0].url });
        return { ...item, url, links };
      },
    }),
    postBuild(),
  ],
  vite: { build: { assetsInlineLimit: 0 }, server: { fs: { allow: [".."] } } },
});
