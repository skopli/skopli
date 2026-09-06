import type { APIRoute, GetStaticPaths } from "astro";
import { getCollection } from "astro:content";

// Raw Markdown endpoints for every docs page: append `.md` to any doc URL to
// get the frontmatter title/description plus the page body as plain Markdown.
// Mirrors the OpenRouter/Stripe `.md` convention. Hand-rolled (no plugin) so
// the dependency surface stays at zero.

export const getStaticPaths: GetStaticPaths = async () => {
  const docs = await getCollection("docs");
  return docs
    .filter((entry) => entry.id !== "index")
    .map((entry) => ({
      params: { slug: entry.id },
      props: { entry },
    }));
};

export const GET: APIRoute = async ({ props }) => {
  const entry = props.entry as Awaited<ReturnType<typeof getCollection>>[number];
  const title = entry.data.title ?? "";
  const description = entry.data.description ?? "";
  const heading = `# ${title}\n\n`;
  const lede = description ? `> ${description}\n\n` : "";
  const body = (entry.body ?? "").trimEnd();
  const markdown = `${heading}${lede}${body}\n`;

  return new Response(markdown, {
    headers: {
      "content-type": "text/markdown; charset=utf-8",
    },
  });
};
