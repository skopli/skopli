import sitemap from "@astrojs/sitemap";
import { project } from "@skopli/ui/project";
import { defineConfig } from "astro/config";

export default defineConfig({
  site: project.site,
  integrations: [sitemap()],
});
