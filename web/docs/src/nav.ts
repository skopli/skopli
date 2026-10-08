import type { StringKey } from "@skopli/ui/i18n";

export interface NavItem {
  slug: string;
  title: StringKey;
}

export interface NavGroup {
  label: StringKey;
  items: NavItem[];
}

export const nav: NavGroup[] = [
  {
    label: "navGuide",
    items: [
      { slug: "guide/getting-started", title: "navGettingStarted" },
      { slug: "guide/reading", title: "navReading" },
      { slug: "guide/rollups", title: "navRollups" },
      { slug: "guide/pricing-basics", title: "navPricingBasics" },
    ],
  },
  {
    label: "navPricing",
    items: [
      { slug: "pricing/model-matching", title: "navModelMatching" },
      { slug: "pricing/cost-math", title: "navCostMath" },
      { slug: "pricing/long-context-tiers", title: "navLongContextTiers" },
      { slug: "pricing/rollups-vs-events", title: "navRollupsVsEvents" },
    ],
  },
  {
    label: "navReference",
    items: [
      { slug: "reference/harnesses", title: "navHarnesses" },
      { slug: "reference/coverage", title: "navCoverage" },
      { slug: "reference/api", title: "navApi" },
    ],
  },
];

export const navItems: NavItem[] = nav.flatMap((group) => group.items);
