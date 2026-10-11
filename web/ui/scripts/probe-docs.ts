import { check, finish, launch } from "./probe.ts";
import "../src/analytics.ts";

const base = process.argv[2] ?? "http://localhost:4321/skopli/";
const { browser, page } = await launch();
await page.context().grantPermissions(["clipboard-read", "clipboard-write"]);
const captured: string[] = [];
await page.exposeFunction("__captured", (e: string) => captured.push(e));
await page.addInitScript(() => {
  window.posthog = {
    capture: (e: string) => (window as unknown as { __captured(e: string): void }).__captured(e),
  };
});

await page.goto(`${base}guide/pricing-basics/`);
const html = await page.content();
check(
  "posthog snippet once in head",
  (html.match(/phc_xFoaFpDvwWHvF2wuoUUa6cms8kmBR7NfgCMWyWHT72YL/g) ?? []).length === 1,
);
check(
  "md alternate link",
  (await page.locator('link[rel="alternate"][type="text/markdown"]').getAttribute("href")) ===
    "/skopli/guide/pricing-basics.md",
);

const theme = () => page.evaluate(() => document.documentElement.dataset.theme);
const toggle = page.locator("[data-theme-toggle]");
const before = await theme();
check(
  "theme label names the next theme",
  (await toggle.getAttribute("aria-label")) ===
    `Switch to ${before === "dark" ? "light" : "dark"} theme`,
);
await toggle.click();
check("theme toggle flips", (await theme()) !== before, `${before} -> ${await theme()}`);
check(
  "theme label follows the flip",
  (await toggle.getAttribute("aria-label")) === `Switch to ${before} theme`,
);
check(
  "theme persisted",
  (await page.evaluate(() => localStorage.getItem("theme"))) === (await theme()),
);

await page.locator(".open-in summary").click();
const links = page.locator(".open-in a");
check("six handoff links", (await links.count()) === 6);
const expected = [
  ["open_chatgpt", "https://chatgpt.com/?q="],
  ["open_claude", "https://claude.ai/new?q="],
  ["open_grok", "https://grok.com/?q="],
  ["open_perplexity", "https://www.perplexity.ai/search?q="],
  ["open_google_ai_mode", "https://www.google.com/search?udm=50&q="],
  ["open_cursor", "https://cursor.com/link/prompt?text="],
];
const prompt = encodeURIComponent(
  "Read https://docs.skopli.com/skopli/guide/pricing-basics.md so I can ask questions about it",
);
for (const [i, [event, prefix]] of expected.entries()) {
  const a = links.nth(i);
  const attrs = await a.evaluate((el) => ({
    href: el.getAttribute("href"),
    event: el.dataset.event,
    target: el.getAttribute("target"),
    rel: el.getAttribute("rel"),
  }));
  check(
    `${event} href/target/rel`,
    attrs.href === prefix + prompt &&
      attrs.event === event &&
      attrs.target === "_blank" &&
      attrs.rel === "noopener",
  );
}
await page.evaluate(() => {
  for (const a of document.querySelectorAll<HTMLAnchorElement>(".open-in a"))
    a.addEventListener("click", (e) => e.preventDefault());
});
await links.nth(0).click();
check("open_chatgpt captured on click", captured.includes("open_chatgpt"));
await page.keyboard.press("Escape");
check(
  "menu closes on Escape",
  !(await page.locator(".open-in").evaluate((el) => (el as HTMLDetailsElement).open)),
);

await page.getByRole("button", { name: "Copy as Markdown" }).click();
await page.waitForFunction(
  () => document.querySelector("[data-copy-md] span")?.textContent === "Copied",
);
const clip = await page.evaluate(() => navigator.clipboard.readText());
check("copy writes markdown", clip.startsWith("# Pricing basics"));
check("docs_copy_md captured after copy", captured.includes("docs_copy_md"));

await page.keyboard.press("/");
await page.fill("#search-input", "priced false");
await page.waitForSelector(".search-results li", { timeout: 5000 }).catch(() => undefined);
check("search returns results", (await page.locator(".search-results li").count()) > 0);
check(
  "search dialog titled result",
  ((await page.locator(".search-results strong").first().textContent())?.length ?? 0) > 0,
);
await page.keyboard.press("Escape");

await page.goto(`${base}guide/getting-started/`);
const outer = page.locator(".tabs").first();
const tabs = outer.locator(':scope > .tablist [role="tab"]');
const shown = () => outer.locator(":scope > .tab-panel:not([hidden])").getAttribute("data-label");
check("tabs rendered", (await tabs.count()) === 9);
await tabs.nth(1).click();
check("tab switch shows Python", (await shown()) === "Python");
await page.reload();
check("tab choice persists", (await shown()) === "Python");
await tabs.nth(1).focus();
await page.keyboard.press("End");
check("End key selects last tab", (await shown()) === (await tabs.last().textContent())?.trim());
await page.keyboard.press("Home");
check("Home key selects first tab", (await shown()) === (await tabs.first().textContent())?.trim());
check(
  "tab ids are deterministic",
  !/[a-z0-9]{5}$/.test((await tabs.first().getAttribute("id")) ?? "") &&
    ((await tabs.first().getAttribute("id")) ?? "").endsWith("-0"),
);
await page.keyboard.press("Tab");
check(
  "focus-visible outline on first tab stop",
  await page.evaluate(() => getComputedStyle(document.activeElement!).outlineStyle !== "none"),
);

await page.setViewportSize({ width: 390, height: 900 });
const sidebarToggle = page.locator(".sidebar-toggle");
const sidebarNav = page.locator("#sidebar-nav");
check("sidebar nav hidden by default at 390", !(await sidebarNav.isVisible()));
await sidebarToggle.click();
check(
  "sidebar toggle opens nav",
  (await sidebarNav.isVisible()) && (await sidebarToggle.getAttribute("aria-expanded")) === "true",
);
check("one sidebar nav in the DOM", (await page.locator("nav.sidebar-nav").count()) === 1);

for (const w of [390, 768, 1440]) {
  await page.setViewportSize({ width: w, height: 900 });
  const small = await page.evaluate(() =>
    [...document.querySelectorAll<HTMLElement>("a, button, summary, input")]
      .filter(
        (el) => el.offsetParent !== null && !el.closest(".prose p, .prose li, .prose td, .ledger"),
      )
      .map((el) => ({ el: el.tagName + "." + el.className, h: el.getBoundingClientRect().height }))
      .filter((x) => x.h > 0 && x.h < 44),
  );
  check(
    `44px targets at ${w}`,
    small.length === 0,
    small.map((x) => `${x.el}:${Math.round(x.h)}`).join(", "),
  );
}

const missing = await page.goto(`${base}no-such-page/`);
check("404 status", missing?.status() === 404, String(missing?.status()));
check(
  "404 has no markdown twin or page actions",
  (await page.locator('link[rel="alternate"], [data-copy-md], [data-event]').count()) === 0,
);

await finish(browser);
