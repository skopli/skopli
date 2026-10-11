import { defineConfig } from "astro/config";

export default defineConfig({
  site: "https://skopli.com",
  trailingSlash: "always",
  build: { format: "directory" },
  i18n: { defaultLocale: "en", locales: ["en"], routing: { prefixDefaultLocale: false } },
});
