// Sidebar order. Ids are collection entry ids (file paths without .md); "index" is the docs home.
export const sections: { title: string; pages: string[] }[] = [
  {
    title: "Guide",
    pages: [
      "index",
      "guide/getting-started",
      "guide/reading",
      "guide/rollups",
      "guide/pricing-basics",
    ],
  },
  {
    title: "Pricing",
    pages: [
      "pricing/model-matching",
      "pricing/cost-math",
      "pricing/long-context-tiers",
      "pricing/rollups-vs-events",
    ],
  },
  { title: "Reference", pages: ["reference/harnesses", "reference/api", "reference/coverage"] },
];
export const order = sections.flatMap((s) => s.pages);
export const pathFor = (id: string) => (id === "index" ? "/skopli/" : `/skopli/${id}/`);
export const mdPathFor = (id: string) => (id === "index" ? "/skopli/index.md" : `/skopli/${id}.md`);
