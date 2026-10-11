import { getCollection } from "astro:content";
import { useTranslations, type Key, type Locale } from "@skopli/ui/i18n";

const SECTIONS: { title: Key; slugs: string[] }[] = [
  { title: "sidebar.start", slugs: ["", "guide/getting-started"] },
  { title: "sidebar.guide", slugs: ["guide/reading", "guide/rollups", "guide/pricing-basics"] },
  {
    title: "sidebar.pricing",
    slugs: [
      "pricing/model-matching",
      "pricing/cost-math",
      "pricing/long-context-tiers",
      "pricing/rollups-vs-events",
    ],
  },
  {
    title: "sidebar.reference",
    slugs: ["reference/api", "reference/harnesses", "reference/coverage"],
  },
];

export type Page = { slug: string; title: string };

export const getNav = async (locale: Locale): Promise<{ title: string; pages: Page[] }[]> => {
  const t = useTranslations(locale);
  const docs = await getCollection("docs");
  const page = (slug: string): Page => ({
    slug,
    title: docs.find((d) => d.id === (slug || "index"))?.data.title ?? slug,
  });
  return SECTIONS.map((s) => ({ title: t(s.title), pages: s.slugs.map(page) }));
};

export const getPages = async (locale: Locale): Promise<Page[]> =>
  (await getNav(locale)).flatMap((s) => s.pages);

export const pageHref = (slug: string): string =>
  `${import.meta.env.BASE_URL}${slug ? `${slug}/` : ""}`;
