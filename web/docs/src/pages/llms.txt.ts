import type { APIRoute } from "astro";
import { getCollection } from "astro:content";
import { PAGES } from "../nav.ts";

const SITE = "https://docs.skopli.com/skopli/";

export const GET: APIRoute = async () => {
  const docs = await getCollection("docs");
  const lines = PAGES.map((p) => {
    const entry = docs.find((d) => d.id === (p.slug || "index"));
    return `- [${entry?.data.title ?? p.title}](${SITE}${p.slug || "index"}.md): ${entry?.data.description ?? ""}`;
  });
  const body = [
    "# Skopli",
    "",
    "> An SDK that reads AI coding-agent usage from 40 harnesses offline, rolls it up, and prices it against market catalogs. Models it cannot match return priced: false instead of a guessed rate.",
    "",
    "## Docs",
    "",
    ...lines,
    "",
    `- [Full docs in one file](${SITE}llms-full.txt)`,
    "",
  ].join("\n");
  return new Response(body, { headers: { "content-type": "text/plain; charset=utf-8" } });
};
