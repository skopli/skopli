import type { APIRoute } from "astro";
import { getCollection } from "astro:content";
import { defaultLocale } from "@skopli/ui/i18n";
import { getPages } from "../nav.ts";

const SITE = "https://docs.skopli.com/skopli/";

export const GET: APIRoute = async () => {
  const docs = await getCollection("docs");
  const home = docs.find((d) => d.id === "index");
  const lines = (await getPages(defaultLocale)).map((p) => {
    const entry = docs.find((d) => d.id === (p.slug || "index"));
    return `- [${p.title}](${SITE}${p.slug || "index"}.md): ${entry?.data.description ?? ""}`;
  });
  const body = [
    `# ${home?.data.title ?? "Skopli"}`,
    "",
    `> ${home?.data.description ?? ""}`,
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
