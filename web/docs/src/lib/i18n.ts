import { defaultLocale, isLocale, localizePath, type Locale, translator } from "@skopli/ui/i18n";
import { basePath, siteOrigin } from "../site.ts";
import { locales, translatedSlugs } from "./translated.ts";

export { locales };

export function resolveLocale(value: string | undefined): Locale {
  return isLocale(value) && locales.includes(value) ? value : defaultLocale;
}

export function t(locale: Locale) {
  return translator(locale);
}

/** Localize a path that exists in every locale, such as the generated llms files. */
export function localizedHref(path: string, locale: Locale): string {
  const localized = localizePath(path, locale);
  return `${basePath}${localized === "/" ? "/" : localized.replace(/\/$/, "")}`;
}

/** Localize a content path, falling back to English when the page has no translation. */
export function href(path: string, locale: Locale = defaultLocale): string {
  const slug = path
    .replace(/^\//, "")
    .replace(/\/$/, "")
    .replace(/\.md$/, "")
    .replace(/^index$/, "");
  const target =
    locale !== defaultLocale && !translatedSlugs[locale]?.has(slug) ? defaultLocale : locale;
  return localizedHref(path, target);
}

export function absoluteHref(path: string, locale: Locale = defaultLocale): string {
  return `${siteOrigin}${href(path, locale)}`;
}
