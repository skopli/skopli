import type { APIRoute } from "astro";
import { getCollection } from "astro:content";
import { ordered } from "../nav";

export const GET: APIRoute = async () => {
  const docs = await getCollection("docs");
  const byId = new Map(docs.map((d) => [d.id, d]));
  const sections = ordered.flatMap(({ slug }) => {
    const entry = byId.get(slug);
    return entry
      ? [`# ${entry.data.title}\n\n> ${entry.data.description}\n\n${entry.body ?? ""}`]
      : [];
  });
  return new Response(sections.join("\n\n---\n\n") + "\n", {
    headers: { "content-type": "text/plain; charset=utf-8" },
  });
};
