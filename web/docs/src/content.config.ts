import { glob } from "astro/loaders";
import { defineCollection } from "astro:content";
import { z } from "astro/zod";
import { pseudoLocaleEnabled } from "./site.ts";

const roots = ["src/content/docs", ...(pseudoLocaleEnabled ? ["test/fixtures"] : [])];

export const collections = {
  docs: defineCollection({
    loader: glob({
      base: ".",
      pattern: roots.map((root) => `${root}/**/*.{md,mdx}`),
      generateId: ({ entry }) =>
        entry
          .replace(/^(src\/content\/docs|test\/fixtures)\//, "")
          .replace(/\.mdx?$/, "")
          .replace(/\/index$/, ""),
    }),
    schema: z.object({
      title: z.string(),
      description: z.string(),
    }),
  }),
};
