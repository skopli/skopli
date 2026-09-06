import type { APIRoute } from "astro";
import { getCollection } from "astro:content";

// Every docs page concatenated as one Markdown document, the llms-full.txt
// convention for content-heavy sites. Built from the same content collection
// as the HTML pages and llms.txt so it cannot drift.

const SITE = "https://docs.skopli.com";
const BASE = "/skopli";

export const GET: APIRoute = async () => {
  const docs = await getCollection("docs");
  const pages = docs
    .filter((entry) => entry.id !== "index")
    .sort((a, b) => a.id.localeCompare(b.id));

  const sections = pages.map((entry) => {
    const title = entry.data.title ?? entry.id;
    const description = entry.data.description ?? "";
    const url = `${SITE}${BASE}/${entry.id}`;
    const heading = `# ${title}\n\n`;
    const source = `Source: ${url}\n\n`;
    const lede = description ? `> ${description}\n\n` : "";
    const body = (entry.body ?? "").trimEnd();
    return `${heading}${source}${lede}${body}\n`;
  });

  const preamble = [
    "# Skopli",
    "",
    "> An SDK that reads AI coding-agent usage from 40 harnesses offline and prices it against market catalogs.",
    "",
    "Complete documentation as a single Markdown document. A per-page index lives at /llms.txt.",
    "",
    "---",
    "",
  ].join("\n");

  return new Response(preamble + sections.join("\n---\n\n"), {
    headers: { "content-type": "text/plain; charset=utf-8" },
  });
};
