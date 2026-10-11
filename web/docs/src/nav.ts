type Page = { slug: string; title: string };

export const NAV: { title: string; pages: Page[] }[] = [
  {
    title: "Start",
    pages: [
      { slug: "", title: "Overview" },
      { slug: "guide/getting-started", title: "Getting started" },
    ],
  },
  {
    title: "Guide",
    pages: [
      { slug: "guide/reading", title: "Reading usage" },
      { slug: "guide/rollups", title: "Rollups" },
      { slug: "guide/pricing-basics", title: "Pricing basics" },
    ],
  },
  {
    title: "Pricing in depth",
    pages: [
      { slug: "pricing/model-matching", title: "Model matching" },
      { slug: "pricing/cost-math", title: "Cost math" },
      { slug: "pricing/long-context-tiers", title: "Long-context tiers" },
      { slug: "pricing/rollups-vs-events", title: "Rollups vs events" },
    ],
  },
  {
    title: "Reference",
    pages: [
      { slug: "reference/api", title: "API" },
      { slug: "reference/harnesses", title: "Harnesses" },
      { slug: "reference/coverage", title: "Coverage" },
    ],
  },
];

export const PAGES = NAV.flatMap((s) => s.pages);
export const pageHref = (slug: string): string =>
  `${import.meta.env.BASE_URL}${slug ? `${slug}/` : ""}`;
