import type { APIRoute } from "astro";
import { getCollection } from "astro:content";
import { pageMarkdown } from "../markdown";
import { ordered } from "../nav";

export const GET: APIRoute = async () => {
  const docs = await getCollection("docs");
  const byId = new Map(docs.map((d) => [d.id, d]));
  const sections = ordered.flatMap(({ slug }) => {
    const entry = byId.get(slug);
    return entry ? [pageMarkdown(entry.data.title, entry.data.description, entry.body)] : [];
  });
  return new Response(sections.join("\n---\n\n"), {
    headers: { "content-type": "text/plain; charset=utf-8" },
  });
};
