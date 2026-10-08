import type { APIRoute, GetStaticPaths } from "astro";
import { locales } from "../../lib/i18n.ts";
import { llmsIndex } from "../../lib/llms.ts";

export const getStaticPaths = (() =>
  locales.map((locale) => ({
    params: { locale: locale === "en" ? undefined : locale },
    props: { locale },
  }))) satisfies GetStaticPaths;

export const GET: APIRoute = async ({ props }) =>
  new Response(await llmsIndex(props.locale), {
    headers: { "content-type": "text/plain; charset=utf-8" },
  });
