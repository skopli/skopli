import {
  configuredLocales,
  defaultLocale,
  localizePath,
  type Locale,
  translator,
} from "@skopli/ui/i18n";
import { pseudoLocaleEnabled, siteOrigin } from "../site.ts";

export const locales = configuredLocales(pseudoLocaleEnabled);

export function t(locale: Locale) {
  return translator(locale);
}

export function href(path: string, locale: Locale = defaultLocale): string {
  return localizePath(path, locale);
}

export function absoluteHref(path: string, locale: Locale = defaultLocale): string {
  const localized = href(path, locale);
  return `${siteOrigin}${localized === "/" ? "/" : `${localized}/`}`;
}
