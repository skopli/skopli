import { defineConfig } from "astro/config";

export default defineConfig({
  site: "https://skopli.com",
  trailingSlash: "always",
  i18n: { defaultLocale: "en", locales: ["en"], routing: { prefixDefaultLocale: false } },
  vite: { build: { assetsInlineLimit: 0 } },
});
