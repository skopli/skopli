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

export function href(path: string, locale: Locale = defaultLocale): string {
  const slug = path.replace(/^\//, "").replace(/\/$/, "");
  const target = locale !== defaultLocale && !translatedSlugs.has(slug) ? defaultLocale : locale;
  const localized = localizePath(path, target);
  return `${basePath}${localized === "/" ? "/" : localized.replace(/\/$/, "")}`;
}

export function absoluteHref(path: string, locale: Locale = defaultLocale): string {
  return `${siteOrigin}${href(path, locale)}`;
}
