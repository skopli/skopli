import type { APIRoute, GetStaticPaths } from "astro";
import { getCollection } from "astro:content";
import { ogImage } from "@skopli/ui/og";
import { project } from "@skopli/ui/project";
import { SITE, pagePath } from "../../nav";

export const getStaticPaths: GetStaticPaths = async () => {
  const docs = await getCollection("docs");
  return docs.map((entry) => ({ params: { slug: entry.id }, props: { entry } }));
};

export const GET: APIRoute = async ({ props }) => {
  const { entry } = props;
  const png = await ogImage({
    site: entry.data.title === project.docsName ? project.name : project.docsName,
    title: entry.data.title,
    description: entry.data.description,
    url: `${SITE}${pagePath(entry.id)}`,
  });
  return new Response(png, { headers: { "content-type": "image/png" } });
};
