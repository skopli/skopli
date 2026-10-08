import { configuredLocales, defaultLocale, localizePath, type Locale } from "@skopli/ui/i18n";
import { pseudoLocaleEnabled, siteOrigin } from "../site.ts";

export const locales = configuredLocales(pseudoLocaleEnabled);

export function localHref(path: string, locale: Locale = defaultLocale): string {
  const localized = localizePath(path, locale);
  return localized === "/" ? "/" : `${localized}/`;
}

export function absoluteHref(path: string, locale: Locale = defaultLocale): string {
  return `${siteOrigin}${localHref(path, locale)}`;
}
