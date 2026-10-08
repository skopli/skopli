import { globSync } from "node:fs";
import { configuredLocales, defaultLocale } from "@skopli/ui/i18n";
import { pseudoLocaleEnabled } from "../site.ts";

export const locales = configuredLocales(pseudoLocaleEnabled);

/** Slugs with a page in every configured non-default locale; other links and alternates stay on English. */
export const translatedSlugs = new Set(
  locales
    .filter((l) => l !== defaultLocale)
    .flatMap((l) =>
      globSync([`src/content/docs/${l}/**/*.{md,mdx}`, `test/fixtures/${l}/**/*.{md,mdx}`]).map(
        (file) =>
          file
            .replace(/^.*?\/[a-z]{2}(-[A-Z]{2})?\//, "")
            .replace(/\.mdx?$/, "")
            .replace(/^index$/, "")
            .replace(/\/index$/, ""),
      ),
    ),
);
