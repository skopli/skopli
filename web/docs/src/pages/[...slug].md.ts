import type { APIRoute, GetStaticPaths } from "astro";
import { toMarkdown } from "../lib/markdown.ts";
import { allDocs, markdownPage, routeSlug, splitId, type Doc } from "../lib/pages.ts";

export const getStaticPaths = (async () =>
  (await allDocs()).map((entry) => {
    const { locale, slug } = splitId(entry.id);
    return {
      params: {
        slug: slug
          ? routeSlug(locale, slug)
          : `${routeSlug(locale, "") ?? ""}/index`.replace(/^\//, ""),
      },
      props: { entry },
    };
  })) satisfies GetStaticPaths;

export const GET: APIRoute = ({ props }) =>
  new Response(toMarkdown(markdownPage(props.entry as Doc)), {
    headers: { "content-type": "text/markdown; charset=utf-8" },
  });
