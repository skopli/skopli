// @ts-check
import sitemap from "@astrojs/sitemap";
import { configuredLocales, defaultLocale } from "@skopli/ui/i18n";
import { defineConfig } from "astro/config";
import { pseudoLocaleEnabled, siteOrigin } from "./src/site.ts";

const locales = configuredLocales(pseudoLocaleEnabled);

export default defineConfig({
  site: siteOrigin,
  i18n: {
    locales,
    defaultLocale,
    routing: { prefixDefaultLocale: false, redirectToDefaultLocale: false },
  },
  integrations: [
    sitemap({
      i18n: { defaultLocale, locales: Object.fromEntries(locales.map((l) => [l, l])) },
      serialize: (item) => {
        const links = item.links ?? [];
        const home = links.find((l) => l.lang === defaultLocale);
        return home ? { ...item, links: [...links, { lang: "x-default", url: home.url }] } : item;
      },
    }),
  ],
  vite: { build: { assetsInlineLimit: 0 }, server: { fs: { allow: [".."] } } },
});
