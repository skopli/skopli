import type { APIRoute, GetStaticPaths } from "astro";
import { getCollection } from "astro:content";
import { pageMarkdown } from "../markdown";

export const getStaticPaths: GetStaticPaths = async () => {
  const docs = await getCollection("docs");
  return docs.map((entry) => ({ params: { slug: entry.id }, props: { entry } }));
};

export const GET: APIRoute = ({ props }) => {
  const { entry } = props;
  return new Response(pageMarkdown(entry.data.title, entry.data.description, entry.body), {
    headers: { "content-type": "text/markdown; charset=utf-8" },
  });
};
