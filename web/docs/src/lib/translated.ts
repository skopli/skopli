import { globSync } from "node:fs";
import { configuredLocales, defaultLocale } from "@skopli/ui/i18n";
import { pseudoLocaleEnabled } from "../site.ts";

export const locales = configuredLocales(pseudoLocaleEnabled);

const slugsIn = (locale: string) =>
  globSync([
    `src/content/docs/${locale}/**/*.{md,mdx}`,
    `test/fixtures/${locale}/**/*.{md,mdx}`,
  ]).map((file) =>
    file
      .replace(/^.*?\/[a-z]{2}(-[A-Z]{2})?\//, "")
      .replace(/\.mdx?$/, "")
      .replace(/^index$/, "")
      .replace(/\/index$/, ""),
  );

/** Slugs with a page in every configured non-default locale; other links and alternates stay on English. */
export const translatedSlugs = locales
  .filter((l) => l !== defaultLocale)
  .map((l) => new Set(slugsIn(l)))
  .reduce(
    (common, set, i) => (i === 0 ? set : new Set([...common].filter((slug) => set.has(slug)))),
    new Set<string>(),
  );
