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
  .map((l) => {
    const slugs = new Set(slugsIn(l));
    if (slugs.size === 0)
      throw new Error(`no ${l} pages under src/content/docs/${l}; run from web/docs`);
    return slugs;
  })
  .reduce(
    (common, set, i) => (i === 0 ? set : new Set([...common].filter((slug) => set.has(slug)))),
    new Set<string>(),
  );
