import AxeBuilder from "@axe-core/playwright";
import { expect, test, type Page } from "@playwright/test";
import { harnesses } from "@skopli/ui/data/harnesses.ts";
import { rows, totals, usd } from "../src/data/example.ts";

const pages = ["/", "/missing-page"];
const isMobile = (page: Page) => (page.viewportSize()?.width ?? 1440) < 768;
const isDesktop = (page: Page) => (page.viewportSize()?.width ?? 1440) >= 1024;

for (const theme of ["dark", "light"] as const) {
  test.describe(`${theme} theme`, () => {
    test.beforeEach(async ({ page }) =>
      page.addInitScript((value) => localStorage.setItem("skopli-theme", value), theme),
    );

    for (const path of pages) {
      test(`${path} has no horizontal overflow and no axe violations`, async ({ page }) => {
        await page.goto(path);
        await expect(page.locator("html")).toHaveAttribute("data-theme", theme);
        const [scrollWidth, clientWidth] = await page.evaluate(() => [
          document.documentElement.scrollWidth,
          document.documentElement.clientWidth,
        ]);
        expect(scrollWidth, "horizontal overflow").toBeLessThanOrEqual(clientWidth);
        const results = await new AxeBuilder({ page })
          .withTags(["wcag2a", "wcag2aa", "wcag21aa"])
          .analyze();
        expect(
          results.violations.map(
            (v) => `${v.id}: ${v.nodes.map((n) => n.target.join(" ")).join(", ")}`,
          ),
        ).toEqual([]);
      });
    }
  });
}

test("skip link is the first tab stop and moves focus to main", async ({ page }) => {
  await page.goto("/");
  await page.keyboard.press("Tab");
  const skip = page.locator(".skip-link");
  await expect(skip).toBeFocused();
  await expect(skip).toBeInViewport();
  await page.keyboard.press("Enter");
  await expect(page.locator("#main")).toBeFocused();
});

test("focus-visible rings render on keyboard focus", async ({ page }) => {
  await page.goto("/");
  await page.keyboard.press("Tab");
  await page.keyboard.press("Tab");
  const outline = await page.evaluate(() => {
    const style = getComputedStyle(document.activeElement as HTMLElement);
    return { style: style.outlineStyle, width: Number.parseFloat(style.outlineWidth) };
  });
  expect(outline.style).not.toBe("none");
  expect(outline.width).toBeGreaterThan(0);
});

test("reduced motion disables transitions", async ({ page }) => {
  await page.emulateMedia({ reducedMotion: "reduce" });
  await page.goto("/");
  const durations = await page.evaluate(() =>
    [".theme-toggle", ".tabs__strip [role=tab]", ".button"].map(
      (sel) => getComputedStyle(document.querySelector(sel)!).transitionDuration,
    ),
  );
  for (const d of durations) expect(Number.parseFloat(d)).toBeLessThanOrEqual(0.01);
});

test("coarse pointers get 44px targets", async ({ page }) => {
  test.skip(!isMobile(page), "touch project only");
  await page.goto("/");
  const boxes = await page.evaluate(() =>
    [
      ...document.querySelectorAll(
        ".site-header a, .site-header button, .tabs__strip [role=tab], .code-frame button, .button, .harnesses__more a, .library__links a, .site-footer a",
      ),
    ]
      .filter((el) => (el as HTMLElement).offsetParent !== null)
      .map((el) => {
        const r = el.getBoundingClientRect();
        return { tag: `${el.tagName}.${el.className}`, w: r.width, h: r.height };
      }),
  );
  expect(boxes.length).toBeGreaterThan(5);
  for (const b of boxes) expect(Math.min(b.w, b.h), JSON.stringify(b)).toBeGreaterThanOrEqual(44);
});

test("header nav hides below the wide breakpoint and scrolls to sections above it", async ({
  page,
}) => {
  await page.goto("/");
  const nav = page.locator(".site-header__nav");
  if (!isDesktop(page)) {
    await expect(nav).toBeHidden();
    return;
  }
  await nav.getByRole("link", { name: "Harnesses" }).click();
  await expect(page).toHaveURL(/#harnesses$/);
  await expect(page.locator("#harnesses")).toBeInViewport();
});

test("the ledger recomputes from the example rows and stays inside its frame", async ({ page }) => {
  await page.goto("/");
  const ledger = page.locator(".ledger");
  await expect(ledger.locator("tfoot td").last()).toHaveText("$211.39");
  expect(usd(totals.usd)).toBe("$211.39");
  await expect(ledger.locator("tbody tr")).toHaveCount(rows.length);
  await expect(ledger.locator(".ledger__miss")).toHaveText("priced: false");
  const report = await page.evaluate(() => {
    const frame = document.querySelector(".ledger .table-frame")!;
    const scroll = frame.querySelector(".table-scroll")!;
    return {
      right: frame.getBoundingClientRect().right,
      overflowing: scroll.scrollWidth > scroll.clientWidth + 1,
      flagged: frame.getAttribute("data-overflow") === "true",
    };
  });
  expect(report.right).toBeLessThanOrEqual(page.viewportSize()!.width);
  expect(report.flagged).toBe(report.overflowing);
});

test("install and quickstart tabs sync by language and persist", async ({ page }) => {
  await page.goto("/");
  const groups = page.locator("skopli-tabs[data-sync-key=lang]");
  await expect(groups).toHaveCount(2);
  await groups.first().getByRole("tab", { name: "Python" }).click();
  for (const group of await groups.all()) {
    await expect(group.getByRole("tab", { name: "Python" })).toHaveAttribute(
      "aria-selected",
      "true",
    );
  }
  await expect(page.locator("#quickstart .code-frame pre:visible")).toContainText("import skopli");
  await page.reload();
  await expect(groups.last().getByRole("tab", { name: "Python" })).toHaveAttribute(
    "aria-selected",
    "true",
  );
  const tab = groups.first().getByRole("tab", { name: "Python" });
  await tab.focus();
  await page.keyboard.press("ArrowRight");
  await expect(groups.first().getByRole("tab", { name: "Rust" })).toBeFocused();
});

test("copy buttons copy the visible code", async ({ page, context }) => {
  test.skip(isMobile(page), "clipboard permission is desktop only");
  await context.grantPermissions(["clipboard-read", "clipboard-write"]);
  await page.goto("/");
  const frame = page.locator("#install .code-frame:visible").first();
  await frame.locator("button").click();
  expect(await page.evaluate(() => navigator.clipboard.readText())).toBe("pnpm add skopli");
});

test("every harness in the registry is listed by name and id", async ({ page }) => {
  await page.goto("/");
  const list = page.locator("#harnesses");
  for (const h of harnesses) {
    await expect(list.locator(`[data-harness="${h.id}"]`)).toContainText(h.name);
    await expect(list.locator(`[data-harness="${h.id}"] code`)).toHaveText(h.id);
  }
});

test("theme persists and is applied before first paint", async ({ page }) => {
  await page.goto("/");
  await expect(page.locator("html")).toHaveAttribute("data-theme", "dark");
  await page.locator(".theme-toggle").first().click();
  await expect(page.locator("html")).toHaveAttribute("data-theme", "light");
  await page.reload();
  await expect(page.locator("html")).toHaveAttribute("data-theme", "light");
  const html = await (await page.request.get("/")).text();
  const themeScript = html.indexOf("skopli-theme");
  const firstStylesheet = html.indexOf('<link rel="stylesheet"');
  expect(themeScript).toBeGreaterThan(-1);
  expect(firstStylesheet).toBeGreaterThan(themeScript);
});

test("footer aligns to the page grid", async ({ page }) => {
  await page.goto("/");
  const [header, section, footer] = await page.evaluate(() => [
    document.querySelector(".site-header__in")!.getBoundingClientRect().left,
    document.querySelector(".section")!.getBoundingClientRect().left,
    document.querySelector(".site-footer__in")!.getBoundingClientRect().left,
  ]);
  expect(Math.abs(header - footer)).toBeLessThanOrEqual(1);
  expect(Math.abs(header - section)).toBeLessThanOrEqual(1);
});

test("cumulative layout shift stays under 0.01", async ({ page }) => {
  await page.addInitScript(() => {
    (window as unknown as { __cls: number }).__cls = 0;
    new PerformanceObserver((list) => {
      for (const entry of list.getEntries() as (PerformanceEntry & {
        hadRecentInput: boolean;
        value: number;
      })[]) {
        if (!entry.hadRecentInput) (window as unknown as { __cls: number }).__cls += entry.value;
      }
    }).observe({ type: "layout-shift", buffered: true });
  });
  await page.goto("/");
  await page.waitForLoadState("networkidle");
  await page.waitForTimeout(500);
  expect(await page.evaluate(() => (window as unknown as { __cls: number }).__cls)).toBeLessThan(
    0.01,
  );
});

test("metadata carries SoftwareSourceCode JSON-LD and the analytics contract", async ({ page }) => {
  await page.goto("/");
  await page.waitForLoadState("networkidle");
  expect(
    await page.evaluate(() => typeof (window as unknown as { posthog?: unknown }).posthog),
  ).toBe("undefined");
  const graph: Record<string, unknown>[] = await page
    .locator('script[type="application/ld+json"]')
    .evaluate((el) => JSON.parse(el.textContent ?? "{}")["@graph"]);
  const jsonLd = graph.find((node) => node["@type"] === "SoftwareSourceCode")!;
  expect(jsonLd).toBeDefined();
  expect(jsonLd.codeRepository).toBe("https://github.com/skopli/skopli");
  expect(jsonLd.license).toBe("https://github.com/skopli/skopli/blob/main/LICENSE");
  expect(jsonLd.programmingLanguage).toHaveLength(9);
  const html = await (await page.request.get("/")).text();
  for (const needle of [
    "phc_xFoaFpDvwWHvF2wuoUUa6cms8kmBR7NfgCMWyWHT72YL",
    "https://eu.i.posthog.com",
    'persistence:"memory"',
    "autocapture:false",
    "capture_pageview:false",
    "disable_session_recording:true",
    "disable_surveys:true",
    'person_profiles:"never"',
    'ph.capture("$pageview")',
    'host==="localhost"||host==="127.0.0.1"||host==="[::1]"||host.endsWith(".local")',
  ]) {
    expect(html.replace(/\s+/g, ""), needle).toContain(needle.replace(/\s+/g, ""));
  }
});

test("404 page renders inside the shared shell", async ({ page }) => {
  const response = await page.goto("/does-not-exist");
  expect(response?.status()).toBe(404);
  await expect(page.getByRole("heading", { level: 1 })).toHaveText("Page not found");
  await expect(page.locator(".site-header")).toBeVisible();
  await page.getByRole("link", { name: "Back to skopli.com" }).click();
  await expect(page).toHaveURL("/");
});
