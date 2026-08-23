import type { APIRoute } from "astro";
import { getCollection } from "astro:content";

// Flat index of every docs page as `- [Title](url.md): description`, the
// OpenRouter/Cloudflare/Stripe llms.txt convention. Hand-rolled from the
// content collection so no plugin is needed.

const SITE = "https://docs.skopli.com";
const BASE = "/skopli";

export const GET: APIRoute = async () => {
  const docs = await getCollection("docs");
  const pages = docs
    .filter((entry) => entry.id !== "index")
    .sort((a, b) => a.id.localeCompare(b.id));

  const lines = pages.map((entry) => {
    const title = entry.data.title ?? entry.id;
    const description = entry.data.description ?? "";
    const url = `${SITE}${BASE}/${entry.id}.md`;
    return description ? `- [${title}](${url}): ${description}` : `- [${title}](${url})`;
  });

  const body = [
    "# Skopli",
    "",
    "> An SDK that reads AI coding-agent usage from 40 harnesses offline and prices it against market catalogs.",
    "",
    "## Docs",
    "",
    ...lines,
    "",
  ].join("\n");

  return new Response(body, {
    headers: { "content-type": "text/plain; charset=utf-8" },
  });
};
