import { check, finish, launch } from "./probe.ts";

const base = process.argv[2] ?? "http://localhost:4321/";
const { browser, page } = await launch();

await page.goto(base);
const html = await page.content();
check("no analytics snippet on the marketing site", !html.includes("posthog"));
check(
  "canonical is https://skopli.com/",
  html.includes('<link rel="canonical" href="https://skopli.com/">'),
);
check("hero ledger has 4 model rows", (await page.locator(".priced tbody tr").count()) === 4);
check("one unpriced row", (await page.locator(".priced .miss").count()) === 1);
const total = await page.locator(".priced tfoot td:last-child").textContent();
check("total excludes the miss", total?.trim() === "$1.86", total ?? "");
check("40 harnesses listed", (await page.locator(".harnesses li").count()) === 40);
check(
  "9 language tabs",
  (await page.locator('.tabs[data-sync="lang"] > .tablist button').count()) === 9,
);
await page.locator('.tabs[data-sync="lang"] > .tablist button', { hasText: "Python" }).click();
check(
  "Python tab shows pip install",
  (
    await page.locator('.tabs[data-sync="lang"] > .tab-panel:not([hidden])').textContent()
  )?.includes("pip install skopli") ?? false,
);

await page.locator("[data-theme-toggle]").click();
check("theme toggle switches to dark", (await page.getAttribute("html", "data-theme")) === "dark");
await page.reload();
check("theme persists across reload", (await page.getAttribute("html", "data-theme")) === "dark");

for (const w of [390, 768, 1440]) {
  await page.setViewportSize({ width: w, height: 900 });
  await page.goto(base);
  const overflow = await page.evaluate(
    () => document.documentElement.scrollWidth - document.documentElement.clientWidth,
  );
  check(`no horizontal overflow at ${w}`, overflow <= 0, `${overflow}px`);
  const small = await page.evaluate(() =>
    [...document.querySelectorAll<HTMLElement>("a, button")]
      .filter((el) => el.offsetParent !== null && !el.closest("p, td, dd"))
      .map((el) => ({
        h: el.getBoundingClientRect().height,
        text: el.textContent?.trim().slice(0, 30),
      }))
      .filter((x) => x.h > 0 && x.h < 44),
  );
  check(
    `44px targets at ${w}`,
    small.length === 0,
    small.map((s) => `${s.text}:${s.h}`).join(", "),
  );
}

const rm = await browser.newPage({
  viewport: { width: 1440, height: 900 },
  reducedMotion: "reduce",
});
await rm.goto(base);
const anim = await rm.evaluate(
  () => getComputedStyle(document.querySelector(".priced tbody tr")!).animationName,
);
check("hero motion off under reduced motion", anim === "none", anim);

await rm.goto(`${base}does-not-exist/`);
check("404 page served", (await rm.locator("h1").textContent()) === "Page not found");

await finish(browser);
