import {
  configuredLocales,
  defaultLocale,
  isLocale,
  localizePath,
  type Locale,
  translator,
} from "@skopli/ui/i18n";
import { basePath, pseudoLocaleEnabled, siteOrigin } from "../site.ts";

export const locales = configuredLocales(pseudoLocaleEnabled);

/** Slugs with a page in a non-default locale, so links from translated pages fall back to English otherwise. */
const translatedSlugs = new Set(
  Object.keys(
    import.meta.glob(["/src/content/docs/*/**/*.{md,mdx}", "/test/fixtures/*/**/*.{md,mdx}"]),
  )
    .filter((file) => !/\/en\//.test(file))
    .map((file) =>
      file
        .replace(/^.*?\/[a-z]{2}(-[A-Z]{2})?\//, "")
        .replace(/\.mdx?$/, "")
        .replace(/^index$/, "")
        .replace(/\/index$/, ""),
    ),
);

export function resolveLocale(value: string | undefined): Locale {
  return isLocale(value) && locales.includes(value) ? value : defaultLocale;
}

export function t(locale: Locale) {
  return translator(locale);
}

export function href(path: string, locale: Locale = defaultLocale): string {
  const slug = path.replace(/^\//, "").replace(/\/$/, "");
  const target = locale !== defaultLocale && !translatedSlugs.has(slug) ? defaultLocale : locale;
  const localized = localizePath(path, target);
  return `${basePath}${localized === "/" ? "/" : localized.replace(/\/$/, "")}`;
}

export function absoluteHref(path: string, locale: Locale = defaultLocale): string {
  return `${siteOrigin}${href(path, locale)}`;
}
