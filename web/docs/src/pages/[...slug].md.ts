import type { APIRoute, GetStaticPaths } from "astro";
import { getCollection } from "astro:content";
import { pageMarkdown } from "../harness-section.ts";

export const getStaticPaths: GetStaticPaths = async () =>
  (await getCollection("docs")).map((entry) => ({ params: { slug: entry.id }, props: { entry } }));

export const GET: APIRoute = ({ props }) => {
  const { entry } = props as { entry: Awaited<ReturnType<typeof getCollection<"docs">>>[number] };
  return new Response(pageMarkdown(entry), {
    headers: { "content-type": "text/markdown; charset=utf-8" },
  });
};
