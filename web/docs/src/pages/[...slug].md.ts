import type { APIRoute, GetStaticPaths } from "astro";
import { getCollection } from "astro:content";

export const getStaticPaths: GetStaticPaths = async () => {
  const docs = await getCollection("docs");
  return docs.map((entry) => ({ params: { slug: entry.id }, props: { entry } }));
};

export const GET: APIRoute = ({ props }) => {
  const { entry } = props;
  const body = `# ${entry.data.title}\n\n> ${entry.data.description}\n\n${entry.body ?? ""}`;
  return new Response(body, { headers: { "content-type": "text/markdown; charset=utf-8" } });
};
