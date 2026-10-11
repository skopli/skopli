import type { APIRoute } from "astro";
import { getCollection } from "astro:content";
import { BASE, SITE, ordered } from "../nav";

export const GET: APIRoute = async () => {
  const docs = await getCollection("docs");
  const byId = new Map(docs.map((d) => [d.id, d]));
  const lines = ordered.flatMap(({ slug }) => {
    const entry = byId.get(slug);
    return entry
      ? [`- [${entry.data.title}](${SITE}${BASE}/${slug}.md): ${entry.data.description}`]
      : [];
  });
  const body = [
    "# Skopli",
    "",
    "> An MIT SDK that reads AI coding-agent usage from 40 harnesses' local logs, rolls it up and prices it against market catalogs. One Rust core, nine language facades.",
    "",
    "## Docs",
    "",
    ...lines,
    "",
    `- [Full docs in one file](${SITE}${BASE}/llms-full.txt)`,
    "",
  ].join("\n");
  return new Response(body, { headers: { "content-type": "text/plain; charset=utf-8" } });
};
