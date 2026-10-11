import en from "./en.json" with { type: "json" };

export const locales = ["en"] as const;
export type Locale = (typeof locales)[number];
export const defaultLocale: Locale = "en";

type Key = keyof typeof en;
const dictionaries: Record<Locale, Record<Key, string>> = { en };

export function t(locale: Locale, key: Key, vars: Record<string, string> = {}): string {
  const text = dictionaries[locale][key];
  return text.replace(/\{(\w+)\}/g, (match, name: string) => vars[name] ?? match);
}

export function localeFromPath(path: string): Locale {
  const first = path.split("/").find(Boolean);
  return locales.find((l) => l === first) ?? defaultLocale;
}
