export interface NavItem {
  slug: string;
  title: string;
}

export interface NavGroup {
  label: string;
  items: NavItem[];
}

export const nav: NavGroup[] = [
  {
    label: "Guide",
    items: [
      { slug: "guide/getting-started", title: "Getting started" },
      { slug: "guide/reading", title: "Reading usage" },
      { slug: "guide/rollups", title: "Rollups" },
      { slug: "guide/pricing-basics", title: "Pricing basics" },
    ],
  },
  {
    label: "Pricing",
    items: [
      { slug: "pricing/model-matching", title: "Model matching" },
      { slug: "pricing/cost-math", title: "Price tokens and cache writes" },
      { slug: "pricing/long-context-tiers", title: "Long-context tiers" },
      { slug: "pricing/rollups-vs-events", title: "Price rollups or raw events" },
    ],
  },
  {
    label: "Reference",
    items: [
      { slug: "reference/harnesses", title: "Supported harnesses" },
      { slug: "reference/coverage", title: "Coverage" },
      { slug: "reference/api", title: "API reference" },
    ],
  },
];

export const navItems: NavItem[] = nav.flatMap((group) => group.items);
