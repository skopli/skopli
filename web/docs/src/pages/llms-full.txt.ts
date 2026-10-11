import type { APIRoute } from "astro";
import { getCollection } from "astro:content";
import { PAGES } from "../nav.ts";
import { toMarkdown } from "./[...slug].md.ts";

export const GET: APIRoute = async () => {
  const docs = await getCollection("docs");
  const body = PAGES.map((p) => docs.find((d) => d.id === (p.slug || "index")))
    .filter((d) => d !== undefined)
    .map(toMarkdown)
    .join("\n---\n\n");
  return new Response(body, { headers: { "content-type": "text/plain; charset=utf-8" } });
};
