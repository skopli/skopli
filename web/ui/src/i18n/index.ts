import en from "./en.json" with { type: "json" };

export const locales = ["en"] as const;
export type Locale = (typeof locales)[number];
export const defaultLocale: Locale = "en";

type Dict = Record<string, string>;
const dicts: Record<Locale, Dict> = { en };

export type Key = keyof typeof en;
type Params = Record<string, string | number>;

const pseudo = (s: string): string =>
  "[" + s.replace(/\{[^}]+\}|[a-zA-Z]/g, (ch) => (ch.length > 1 ? ch : (PSEUDO[ch] ?? ch))) + "]";

const PSEUDO: Dict = {
  a: "á",
  e: "é",
  i: "í",
  o: "ó",
  u: "ú",
  y: "ý",
  c: "ç",
  n: "ñ",
  s: "š",
  z: "ž",
  A: "Á",
  E: "É",
  I: "Í",
  O: "Ó",
  U: "Ú",
  Y: "Ý",
  C: "Ç",
  N: "Ñ",
  S: "Š",
  Z: "Ž",
};

export const pseudoLocale = process.env.SKOPLI_PSEUDO_LOCALE === "1";

export const translate = (locale: Locale, key: Key, params?: Params): string => {
  const raw = dicts[locale][key] ?? dicts[defaultLocale][key] ?? key;
  const text = raw.replace(/\{(\w+)\}/g, (m, name: string) => String(params?.[name] ?? m));
  return pseudoLocale ? pseudo(text) : text;
};

export const useTranslations = (locale: Locale) => (key: Key, params?: Params) =>
  translate(locale, key, params);

export const localeFromPath = (pathname: string, base: string): Locale => {
  const first = pathname.slice(base.length).split("/")[0];
  return (locales as readonly string[]).includes(first ?? "") ? (first as Locale) : defaultLocale;
};
