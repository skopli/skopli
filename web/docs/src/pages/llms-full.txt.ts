import type { APIRoute } from "astro";
import { getCollection } from "astro:content";
import { order } from "../nav.ts";

export const GET: APIRoute = async () => {
  const entries = await getCollection("docs");
  const text = order
    .map((id) => entries.find((e) => e.id === id))
    .filter((e) => e !== undefined)
    .map((e) => `# ${e.data.title}\n\n${e.data.description}\n\n${e.body ?? ""}`)
    .join("\n\n---\n\n");
  return new Response(text, { headers: { "content-type": "text/plain; charset=utf-8" } });
};
