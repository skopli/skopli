import type { APIRoute, GetStaticPaths } from "astro";
import { getCollection, type CollectionEntry } from "astro:content";

export const toMarkdown = (entry: CollectionEntry<"docs">): string =>
  `# ${entry.data.title}\n\n> ${entry.data.description}\n\n${(entry.body ?? "").replace(/^import .*$\n?/gm, "").trim()}\n`;

export const getStaticPaths: GetStaticPaths = async () =>
  (await getCollection("docs")).map((entry) => ({ params: { slug: entry.id }, props: { entry } }));

export const GET: APIRoute<{ entry: CollectionEntry<"docs"> }> = ({ props }) =>
  new Response(toMarkdown(props.entry), {
    headers: { "content-type": "text/markdown; charset=utf-8" },
  });
