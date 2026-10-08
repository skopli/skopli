import { pseudolocalize } from "./pseudo.ts";
import { en, type StringKey, type Strings } from "./strings.ts";

export { pseudolocalize } from "./pseudo.ts";
export type { StringKey, Strings } from "./strings.ts";

export const defaultLocale = "en";
export const pseudoLocale = "en-XA";
export const productionLocales = [defaultLocale] as const;

export type Locale = (typeof productionLocales)[number] | typeof pseudoLocale;

export const localeNames: Record<Locale, string> = {
  en: "English",
  "en-XA": "Pseudo (en-XA)",
};

export function configuredLocales(pseudo: boolean): Locale[] {
  return pseudo ? [...productionLocales, pseudoLocale] : [...productionLocales];
}

const translations: Record<Locale, Strings> = {
  en,
  "en-XA": Object.fromEntries(
    Object.entries(en).map(([key, value]) => [key, pseudolocalize(value)]),
  ) as Strings,
};

export function isLocale(value: string | undefined): value is Locale {
  return value !== undefined && value in translations;
}

export function format(template: string, params: Record<string, string | number> = {}): string {
  return template.replace(/\{(\w+)\}/g, (match, name: string) =>
    name in params ? String(params[name]) : match,
  );
}

export function translator(locale: Locale) {
  const strings = translations[locale];
  return (key: StringKey, params?: Record<string, string | number>) => format(strings[key], params);
}

export type Translate = ReturnType<typeof translator>;

export function localeFromPath(pathname: string, locales: readonly Locale[]): Locale {
  const first = pathname.split("/").find(Boolean);
  return locales.find((l) => l !== defaultLocale && l === first) ?? defaultLocale;
}

export function stripLocale(pathname: string, locale: Locale): string {
  if (locale === defaultLocale) return pathname;
  return pathname.replace(new RegExp(`^/${locale}(?=/|$)`), "") || "/";
}

export function localizePath(pathname: string, locale: Locale): string {
  const bare = pathname.startsWith("/") ? pathname : `/${pathname}`;
  return locale === defaultLocale ? bare : `/${locale}${bare === "/" ? "" : bare}`;
}

export function aiPrompt(mdUrl: string): string {
  return format(en.aiPrompt, { url: mdUrl });
}
