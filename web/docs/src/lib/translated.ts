import { globSync } from "node:fs";
import { configuredLocales, defaultLocale, type Locale, pseudoLocale } from "@skopli/ui/i18n";
import { pseudoLocaleEnabled } from "../site.ts";

export const locales = configuredLocales(pseudoLocaleEnabled);

/** Page slugs with source under src/content/docs/<locale>; the pseudo locale also reads test/fixtures. */
export const slugsIn = (locale: Locale) =>
  globSync([
    `src/content/docs/${locale}/**/*.{md,mdx}`,
    ...(locale === pseudoLocale ? [`test/fixtures/${locale}/**/*.{md,mdx}`] : []),
  ]).map((file) =>
    file
      .replace(/^.*?\/[a-z]{2}(-[A-Z]{2})?\//, "")
      .replace(/\.mdx?$/, "")
      .replace(/^index$/, "")
      .replace(/\/index$/, ""),
  );

/** Per locale, the slugs with a page in that locale; other links and alternates stay on English. */
export const translatedSlugs: Partial<Record<Locale, Set<string>>> = Object.fromEntries(
  locales
    .filter((l) => l !== defaultLocale)
    .map((l) => {
      const slugs = new Set(slugsIn(l));
      if (slugs.size === 0)
        throw new Error(`no ${l} pages under src/content/docs/${l}; run from web/docs`);
      return [l, slugs];
    }),
);
