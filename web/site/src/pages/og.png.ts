import type { APIRoute } from "astro";
import { ogImage } from "@skopli/ui/og";
import { project } from "@skopli/ui/project";
import { description, title } from "../content.ts";

export const GET: APIRoute = async () =>
  new Response(await ogImage({ site: project.name, title, description, url: project.site }), {
    headers: { "content-type": "image/png" },
  });
