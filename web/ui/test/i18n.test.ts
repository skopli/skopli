import { expect, test } from "vitest";
import en from "../src/i18n/en.json" with { type: "json" };
import { localeFromPath, translate } from "../src/i18n/index.ts";

test("prompt template matches the AI handoff contract", () => {
  expect(en["actions.prompt"]).toBe("Read {url} so I can ask questions about it");
  expect(
    translate("en", "actions.prompt", { url: "https://docs.skopli.com/skopli/index.md" }),
  ).toBe("Read https://docs.skopli.com/skopli/index.md so I can ask questions about it");
});

test("every placeholder is interpolated and unknown ones are left visible", () => {
  for (const [key, value] of Object.entries(en)) {
    const params = Object.fromEntries([...value.matchAll(/\{(\w+)\}/g)].map((m) => [m[1], "x"]));
    expect(translate("en", key as keyof typeof en, params)).not.toMatch(/\{\w+\}/);
  }
  expect(translate("en", "footer.copyright")).toBe("Copyright {year} Skopli contributors");
});

test("locale resolves from the path and falls back to English", () => {
  expect(localeFromPath("/skopli/guide/reading/", "/skopli/")).toBe("en");
  expect(localeFromPath("/skopli/", "/skopli/")).toBe("en");
});
