import { defineConfig } from "astro/config";
import { codeThemes } from "@skopli/ui/code-theme.ts";
import { rehypeCodeTabs, tabMetaTransformer } from "@skopli/ui/code-tabs.ts";

export default defineConfig({
  site: "https://docs.skopli.com",
  base: "/skopli/",
  trailingSlash: "always",
  build: { format: "directory" },
  redirects: { "/contributing": "/reference/coverage/" },
  i18n: { defaultLocale: "en", locales: ["en"], routing: { prefixDefaultLocale: false } },
  markdown: {
    shikiConfig: { themes: codeThemes, defaultColor: false, transformers: [tabMetaTransformer] },
    rehypePlugins: [rehypeCodeTabs],
  },
});
