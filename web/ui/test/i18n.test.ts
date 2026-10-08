import { describe, expect, it } from "vitest";
import {
  aiPrompt,
  configuredLocales,
  format,
  localeFromPath,
  localizePath,
  pseudolocalize,
  stripLocale,
  translator,
} from "../i18n/index.ts";
import { en } from "../i18n/strings.ts";

describe("i18n core", () => {
  it("ships English alone in production and adds en-XA for pseudo builds", () => {
    expect(configuredLocales(false)).toEqual(["en"]);
    expect(configuredLocales(true)).toEqual(["en", "en-XA"]);
  });

  it("keeps the AI prompt byte-identical in English", () => {
    expect(aiPrompt("https://docs.skopli.com/skopli/guide/reading.md")).toBe(
      "Read https://docs.skopli.com/skopli/guide/reading.md so I can ask questions about it",
    );
  });

  it("pseudolocalizes every string while preserving placeholders", () => {
    const t = translator("en-XA");
    const text = t("openIn", { tool: "Cursor" });
    expect(text.startsWith("[")).toBe(true);
    expect(text).toContain("Cursor");
    expect(text).not.toBe(en.openIn);
    for (const value of Object.values(en)) {
      const pseudo = pseudolocalize(value);
      for (const placeholder of value.match(/\{\w+\}/g) ?? [])
        expect(pseudo).toContain(placeholder);
      expect(pseudo).not.toBe(value);
    }
  });

  it("formats placeholders and leaves unknown ones alone", () => {
    expect(format("{a} and {b}", { a: 1 })).toBe("1 and {b}");
  });

  it("maps paths to and from locale prefixes", () => {
    expect(localizePath("/guide/reading", "en")).toBe("/guide/reading");
    expect(localizePath("/guide/reading", "en-XA")).toBe("/en-XA/guide/reading");
    expect(localizePath("/", "en-XA")).toBe("/en-XA");
    expect(localeFromPath("/en-XA/guide", ["en", "en-XA"])).toBe("en-XA");
    expect(localeFromPath("/guide", ["en", "en-XA"])).toBe("en");
    expect(stripLocale("/en-XA/guide", "en-XA")).toBe("/guide");
    expect(stripLocale("/en-XA", "en-XA")).toBe("/");
  });
});
