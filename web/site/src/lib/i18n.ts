import { configuredLocales, defaultLocale, localizePath, type Locale } from "@skopli/ui/i18n";
import { pseudoLocaleEnabled, siteOrigin } from "../site.ts";

export const locales = configuredLocales(pseudoLocaleEnabled);

export function absoluteHref(path: string, locale: Locale = defaultLocale): string {
  const localized = localizePath(path, locale);
  return `${siteOrigin}${localized === "/" ? "/" : `${localized}/`}`;
}
