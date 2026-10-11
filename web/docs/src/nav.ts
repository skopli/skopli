export type NavItem = { label: string; slug: string };

export const nav: { label: string; items: NavItem[] }[] = [
  {
    label: "Guide",
    items: [
      { label: "Getting started", slug: "guide/getting-started" },
      { label: "Reading usage", slug: "guide/reading" },
      { label: "Rollups", slug: "guide/rollups" },
      { label: "Pricing basics", slug: "guide/pricing-basics" },
    ],
  },
  {
    label: "Pricing in depth",
    items: [
      { label: "Model matching", slug: "pricing/model-matching" },
      { label: "Cost math", slug: "pricing/cost-math" },
      { label: "Long-context tiers", slug: "pricing/long-context-tiers" },
      { label: "Rollups vs events", slug: "pricing/rollups-vs-events" },
    ],
  },
  {
    label: "Reference",
    items: [
      { label: "Supported harnesses", slug: "reference/harnesses" },
      { label: "Coverage", slug: "reference/coverage" },
      { label: "API reference", slug: "reference/api" },
    ],
  },
];

export const ordered: NavItem[] = [
  { label: "Overview", slug: "index" },
  ...nav.flatMap((g) => g.items),
];

export const SITE = "https://docs.skopli.com";
export const BASE = "/skopli";

export const pagePath = (slug: string): string =>
  slug === "index" ? `${BASE}/` : `${BASE}/${slug}/`;
export const mdPath = (slug: string): string => `${BASE}/${slug}.md`;
