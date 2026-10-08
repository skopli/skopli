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

export function resolveLocale(value: string | undefined): Locale {
  return isLocale(value) && locales.includes(value) ? value : defaultLocale;
}

export function t(locale: Locale) {
  return translator(locale);
}

export function href(path: string, locale: Locale = defaultLocale): string {
  const localized = localizePath(path, locale);
  return `${basePath}${localized === "/" ? "/" : localized.replace(/\/$/, "")}`;
}

export function absoluteHref(path: string, locale: Locale = defaultLocale): string {
  return `${siteOrigin}${href(path, locale)}`;
}
