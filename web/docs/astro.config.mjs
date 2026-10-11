import { defineConfig } from "astro/config";
import mdx from "@astrojs/mdx";

export default defineConfig({
  site: "https://docs.skopli.com",
  base: "/skopli/",
  outDir: "./dist/skopli",
  trailingSlash: "always",
  integrations: [mdx()],
  i18n: { defaultLocale: "en", locales: ["en"], routing: { prefixDefaultLocale: false } },
  vite: { build: { assetsInlineLimit: 0 } },
  markdown: {
    shikiConfig: { themes: { light: "vitesse-light", dark: "vitesse-dark" }, defaultColor: false },
  },
});
