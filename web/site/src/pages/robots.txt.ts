import type { APIRoute } from "astro";
import { project } from "@skopli/ui/project";

export const GET: APIRoute = () =>
  new Response(`User-agent: *\nAllow: /\n\nSitemap: ${project.site}sitemap-index.xml\n`, {
    headers: { "content-type": "text/plain; charset=utf-8" },
  });
