import { defaultLocale, type Locale } from "@skopli/ui/i18n";
import type { MarkdownPage } from "./markdown.ts";
import { basePath, siteOrigin } from "../site.ts";
import { getCollection, type CollectionEntry } from "astro:content";
import { nav, navItems } from "../nav.ts";
import { resolveLocale } from "./i18n.ts";

export type Doc = CollectionEntry<"docs">;

export function splitId(id: string): { locale: Locale; slug: string } {
  const [first, ...rest] = id.split("/");
  const locale = resolveLocale(first);
  if (first !== locale) throw new Error(`doc ${id} is not under a configured locale folder`);
  return { locale, slug: rest.join("/") };
}

export function docPath(locale: Locale, slug: string): string {
  const route = routeSlug(locale, slug);
  return route ? `/${route}${slug ? "" : "/"}` : "/";
}

export function mdPath(slug: string): string {
  return slug ? `/${slug}.md` : "/index.md";
}

export function routeSlug(locale: Locale, slug: string): string | undefined {
  const parts = [locale === defaultLocale ? "" : locale, slug].filter(Boolean);
  return parts.length ? parts.join("/") : undefined;
}

export async function allDocs(): Promise<Doc[]> {
  return getCollection("docs");
}

export async function docsFor(locale: Locale): Promise<Doc[]> {
  return (await allDocs()).filter((doc) => splitId(doc.id).locale === locale);
}

export function orderedDocs(docs: Doc[]): Doc[] {
  const order = new Map(["", ...navItems.map((i) => i.slug)].map((slug, i) => [slug, i]));
  return [...docs].sort(
    (a, b) =>
      (order.get(splitId(a.id).slug) ?? 999) - (order.get(splitId(b.id).slug) ?? 999) ||
      a.id.localeCompare(b.id),
  );
}

export function neighbours(slug: string): {
  prev?: (typeof navItems)[number];
  next?: (typeof navItems)[number];
} {
  const index = navItems.findIndex((item) => item.slug === slug);
  if (slug === "") return { next: navItems[0] };
  if (index < 0) return {};
  return { prev: navItems[index - 1], next: navItems[index + 1] };
}

export function groupFor(slug: string) {
  return nav.find((group) => group.items.some((item) => item.slug === slug));
}

export function markdownPage(doc: Doc): MarkdownPage {
  const { locale, slug } = splitId(doc.id);
  return {
    title: doc.data.title,
    description: doc.data.description,
    body: doc.body ?? "",
    siteBase: `${siteOrigin}${basePath}`,
    path: docPath(locale, slug),
    source: doc.id,
  };
}
