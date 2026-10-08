import AxeBuilder from "@axe-core/playwright";
import { expect, test, type Page } from "@playwright/test";
import { pageActionEvents } from "@skopli/ui/analytics";
import { navItems } from "../src/nav.ts";

const pages = [
  "/skopli/",
  ...navItems.map((i) => `/skopli/${i.slug}`),
  "/skopli/privacy",
  "/skopli/missing-page",
];
const prompt = encodeURIComponent(
  "Read https://docs.skopli.com/skopli/guide/getting-started.md so I can ask questions about it",
);
const aiUrls: Record<string, string> = {
  open_chatgpt: `https://chatgpt.com/?q=${prompt}`,
  open_claude: `https://claude.ai/new?q=${prompt}`,
  open_grok: `https://grok.com/?q=${prompt}`,
  open_perplexity: `https://perplexity.ai/search?q=${prompt}`,
  open_google_ai_mode: `https://google.com/search?udm=50&q=${prompt}`,
  open_cursor: `https://cursor.com/link/prompt?text=${prompt}`,
};

const isMobile = (page: Page) => (page.viewportSize()?.width ?? 1440) < 768;
const isDesktop = (page: Page) => (page.viewportSize()?.width ?? 1440) >= 1024;

async function setTheme(page: Page, theme: "dark" | "light") {
  await page.addInitScript((value) => localStorage.setItem("skopli-theme", value), theme);
}

for (const theme of ["dark", "light"] as const) {
  test.describe(`${theme} theme`, () => {
    test.beforeEach(async ({ page }) => setTheme(page, theme));

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
  await page.goto("/skopli/guide/getting-started");
  await page.keyboard.press("Tab");
  const skip = page.locator(".skip-link");
  await expect(skip).toBeFocused();
  await expect(skip).toBeInViewport();
  await page.keyboard.press("Enter");
  await expect(page.locator("#main")).toBeFocused();
});

test("focus-visible rings render on keyboard focus", async ({ page }) => {
  await page.goto("/skopli/guide/getting-started");
  await page.keyboard.press("Tab");
  await page.keyboard.press("Tab");
  const outline = await page.evaluate(() => {
    const el = document.activeElement as HTMLElement;
    const style = getComputedStyle(el);
    return {
      matches: el.matches(":focus-visible"),
      width: style.outlineWidth,
      style: style.outlineStyle,
    };
  });
  expect(outline.matches).toBe(true);
  expect(outline.style).not.toBe("none");
  expect(Number.parseFloat(outline.width)).toBeGreaterThan(0);
});

test("reduced motion disables transitions", async ({ page }) => {
  await page.emulateMedia({ reducedMotion: "reduce" });
  await page.goto("/skopli/guide/getting-started");
  const durations = await page.evaluate(() =>
    [".theme-toggle", ".tabs__strip [role=tab]", ".sidebar-nav a"].map(
      (sel) => getComputedStyle(document.querySelector(sel)!).transitionDuration,
    ),
  );
  for (const d of durations.flatMap((list) => list.split(",")))
    expect(Number.parseFloat(d)).toBeLessThanOrEqual(0.01);
});

test("coarse pointers get 44px targets", async ({ page }) => {
  test.skip(!isMobile(page), "touch project only");
  await page.goto("/skopli/guide/getting-started");
  const boxes = await page.evaluate(() =>
    [
      ...document.querySelectorAll(
        ".site-header a, .site-header button, .drawer-open, .tabs__strip [role=tab], .page-actions__copy, .page-actions__more, .pager__link, .code-frame button",
      ),
    ]
      .filter((el) => (el as HTMLElement).offsetParent !== null)
      .map((el) => {
        const r = el.getBoundingClientRect();
        return {
          tag: `${el.tagName}.${el.className} ${el.textContent?.trim().slice(0, 20)}`,
          w: r.width,
          h: r.height,
        };
      }),
  );
  expect(boxes.length).toBeGreaterThan(5);
  for (const b of boxes) expect(Math.min(b.w, b.h), JSON.stringify(b)).toBeGreaterThanOrEqual(44);
});

test("tables stay inside their frame and signal overflow", async ({ page }) => {
  await page.goto("/skopli/reference/api");
  const frames = page.locator(".table-frame");
  expect(await frames.count()).toBeGreaterThan(0);
  const report = await page.evaluate(() => {
    const main = document.querySelector("article")!.getBoundingClientRect();
    return [...document.querySelectorAll(".table-frame")].map((f) => {
      const r = f.getBoundingClientRect();
      const scroll = f.querySelector(".table-scroll")!;
      return {
        inside: r.left >= main.left - 1 && r.right <= main.right + 1,
        overflowing: scroll.scrollWidth > scroll.clientWidth + 1,
        flagged: f.getAttribute("data-overflow") === "true",
      };
    });
  });
  for (const r of report) {
    expect(r.inside).toBe(true);
    expect(r.flagged).toBe(r.overflowing);
  }
});

test("code frames hug their content", async ({ page }) => {
  await page.goto("/skopli/guide/getting-started");
  const sizes = await page.evaluate(() =>
    [...document.querySelectorAll(".code-frame pre")].slice(0, 5).map((pre) => ({
      minHeight: getComputedStyle(pre).minHeight,
      lineHeight: parseFloat(getComputedStyle(pre).lineHeight),
      height: pre.getBoundingClientRect().height,
      lines: (pre.textContent ?? "").trimEnd().split("\n").length,
    })),
  );
  for (const s of sizes) {
    expect(["0px", "auto"]).toContain(s.minHeight);
    expect(s.height).toBeLessThan((s.lines + 2) * s.lineHeight);
  }
});

test("tabs sync by key, persist across reload, and nest inside one frame", async ({ page }) => {
  await page.goto("/skopli/guide/getting-started");
  const groups = page.locator("skopli-tabs[data-sync-key=lang]");
  expect(await groups.count()).toBeGreaterThanOrEqual(2);
  await groups.first().getByRole("tab", { name: "Python" }).click();
  for (const group of await groups.all()) {
    await expect(group.getByRole("tab", { name: "Python" })).toHaveAttribute(
      "aria-selected",
      "true",
    );
  }
  await page.reload();
  await expect(groups.first().getByRole("tab", { name: "Python" })).toHaveAttribute(
    "aria-selected",
    "true",
  );
  await groups.first().getByRole("tab", { name: "TypeScript" }).click();
  const nested = page.locator(".tabs__panel > .tabs").first();
  await expect(nested).toBeVisible();
  expect(await nested.evaluate((el) => getComputedStyle(el).borderTopWidth)).toBe("0px");
  const tab = groups.first().getByRole("tab", { name: "TypeScript" });
  await tab.focus();
  await page.keyboard.press("ArrowRight");
  await expect(groups.first().getByRole("tab", { name: "Python" })).toBeFocused();
  await expect(groups.nth(1).getByRole("tab", { name: "Python" })).toHaveAttribute(
    "aria-selected",
    "true",
  );
  expect(await page.evaluate(() => localStorage.getItem("skopli-tab-lang"))).toBe("Python");
});

test("trailing-slash URLs redirect to the extensionless page", async ({ page }) => {
  await page.goto("/skopli/reference/api/");
  await expect(page).toHaveURL(/\/skopli\/reference\/api$/);
  await expect(page.locator("h1")).toHaveText("API reference");
});

test("the drawer carries the locale switch on translated pages", async ({ page }) => {
  test.skip(isDesktop(page), "drawer below the sidebar breakpoint");
  const response = await page.goto("/skopli/en-XA/guide/getting-started");
  test.skip(response?.status() !== 200, "pseudo-locale build only");
  await expect(page.locator(".site-header__actions .locale-switch")).toBeHidden();
  await page.locator(".drawer-open").click();
  await expect(page.locator("dialog#drawer .locale-switch")).toBeVisible();
});

test("the breadcrumb-row actions menu closes once its button slides under the header", async ({
  page,
}) => {
  test.skip(isMobile(page) || isDesktop(page), "tablet only");
  await page.goto("/skopli/guide/rollups");
  const actions = page.locator(".docs__top .page-actions").first();
  await actions.locator(".page-actions__more").click();
  await expect(actions.locator(".page-actions__menu")).toBeVisible();
  await page.mouse.wheel(0, 120);
  await expect(actions.locator(".page-actions__menu")).toBeHidden();
});

test("page actions copy Markdown and link the six AI handoffs", async ({ page, context }) => {
  test.skip(!isDesktop(page), "desktop rail only");
  await context.grantPermissions(["clipboard-read", "clipboard-write"]);
  await page.goto("/skopli/guide/getting-started");
  const actions = page.locator(".docs__rail .page-actions").first();
  await actions.locator(".page-actions__copy").click();
  await expect(actions.locator(".page-actions__copy")).toHaveAttribute("data-state", "copied");
  const clipboard = await page.evaluate(() => navigator.clipboard.readText());
  expect(clipboard.startsWith("# Getting started\n\n> Install Skopli")).toBe(true);
  expect(clipboard).not.toContain("import Tabs");
  expect(clipboard).toContain("**Python**");

  const more = actions.locator(".page-actions__more");
  await more.click();
  const menu = actions.locator(".page-actions__menu");
  await expect(menu).toBeVisible();
  const items = menu.locator("[role=menuitem]");
  await expect(items.first()).toHaveAttribute("href", "/skopli/guide/getting-started.md");
  for (const [event, url] of Object.entries(aiUrls)) {
    const link = menu.locator(`[data-event="${event}"]`);
    await expect(link).toHaveAttribute("href", url);
    await expect(link).toHaveAttribute("target", "_blank");
    await expect(link).toHaveAttribute("rel", "noopener");
  }
  const events = await page
    .locator("[data-event]")
    .evaluateAll((els) => [...new Set(els.map((e) => e.getAttribute("data-event")))].sort());
  expect(events).toEqual(Object.values(pageActionEvents).sort());

  await page.keyboard.press("Escape");
  await expect(menu).toBeHidden();
  await expect(more).toBeFocused();
  await page.keyboard.press("ArrowDown");
  await expect(menu).toBeVisible();
  await expect(items.first()).toBeFocused();
  await page.keyboard.press("ArrowDown");
  await expect(items.nth(1)).toBeFocused();
  await page.keyboard.press("End");
  await expect(items.last()).toBeFocused();
  await page.keyboard.press("ArrowDown");
  await expect(items.first()).toBeFocused();
  await page.keyboard.press("Escape");
  await expect(menu).toBeHidden();
  await expect(more).toBeFocused();
  await more.click();
  await expect(menu).toBeVisible();
  await page.mouse.click(5, 500);
  await expect(menu).toBeHidden();
});

test("page-actions menu stays inside the viewport below the rail breakpoint", async ({ page }) => {
  test.skip(isDesktop(page), "the rail handles desktop");
  await page.goto("/skopli/guide/getting-started");
  const actions = page.locator(".page-actions:visible").first();
  await actions.locator(".page-actions__more").click();
  const menu = actions.locator(".page-actions__menu");
  await expect(menu).toBeVisible();
  const box = (await menu.boundingBox())!;
  const { width, height } = page.viewportSize()!;
  expect(box.x).toBeGreaterThanOrEqual(0);
  expect(box.y).toBeGreaterThanOrEqual(0);
  expect(box.x + box.width).toBeLessThanOrEqual(width);
  expect(box.y + box.height).toBeLessThanOrEqual(height);
});

test("search opens with Ctrl+K, returns Pagefind results, closes on Escape", async ({ page }) => {
  await page.goto("/skopli/");
  await page.keyboard.press("ControlOrMeta+k");
  const dialog = page.locator("dialog.search");
  await expect(dialog).toBeVisible();
  await expect(dialog.locator(".search__input")).toBeFocused();
  await dialog.locator(".search__input").fill("rollup");
  await expect(dialog.locator(".search__results a").first()).toBeVisible();
  for (const url of await dialog
    .locator(".search__results a")
    .evaluateAll((links) => links.map((a) => a.getAttribute("href"))))
    expect(url).toMatch(/^\/skopli\/[^.]*$/);
  const box = await dialog.boundingBox();
  const vw = page.viewportSize()!.width;
  expect(Math.abs(box!.x + box!.width / 2 - vw / 2)).toBeLessThan(2);
  await page.keyboard.press("ArrowDown");
  await expect(dialog.locator(".search__results a").first()).toBeFocused();
  await page.keyboard.press("Escape");
  await expect(dialog).toBeHidden();
});

test("Enter in the search field opens the top result", async ({ page }) => {
  await page.goto("/skopli/");
  await page.keyboard.press("ControlOrMeta+k");
  const dialog = page.locator("dialog.search");
  await dialog.locator(".search__input").fill("rollup");
  const first = dialog.locator(".search__results a").first();
  await expect(first).toBeVisible();
  const href = (await first.getAttribute("href"))!;
  await page.keyboard.press("Enter");
  await expect(page).toHaveURL(new RegExp(`${href.replace(/[.*+?^${}()|[\]\\]/g, "\\$&")}$`));
  await expect(dialog).toBeHidden();
});

test("theme persists and is applied before first paint", async ({ page }) => {
  await page.goto("/skopli/guide/rollups");
  const initial = await page.locator("html").getAttribute("data-theme");
  expect(initial).toBe("dark");
  await page.locator(".theme-toggle").first().click();
  await expect(page.locator("html")).toHaveAttribute("data-theme", "light");
  await page.reload();
  await expect(page.locator("html")).toHaveAttribute("data-theme", "light");
  const html = await (await page.request.get("/skopli/guide/rollups")).text();
  const themeScript = html.indexOf("skopli-theme");
  const firstStylesheet = html.indexOf('<link rel="stylesheet"');
  expect(themeScript).toBeGreaterThan(-1);
  expect(firstStylesheet).toBeGreaterThan(themeScript);
});

test("mobile drawer traps focus and closes on Escape", async ({ page }) => {
  test.skip(isDesktop(page), "drawer below the sidebar breakpoint");
  await page.goto("/skopli/guide/rollups");
  await expect(page.locator(".sidebar-nav").first()).toBeHidden();
  const open = page.locator(".drawer-open");
  await open.click();
  const drawer = page.locator("dialog#drawer");
  await expect(drawer).toBeVisible();
  await expect(open).toHaveAttribute("aria-expanded", "true");
  for (let i = 0; i < 30; i++) {
    await page.keyboard.press("Tab");
    const inside = await page.evaluate(() =>
      document.querySelector("dialog#drawer")!.contains(document.activeElement),
    );
    expect(inside, `tab stop ${i} left the drawer`).toBe(true);
  }
  await page.keyboard.press("Escape");
  await expect(drawer).toBeHidden();
  await expect(open).toBeFocused();
  await open.click();
  await page.mouse.click(380, 800);
  await expect(drawer).toBeHidden();
});

test("medium viewport moves actions to the breadcrumb row and hides the rail", async ({ page }) => {
  test.skip(isMobile(page) || isDesktop(page), "tablet only");
  await page.goto("/skopli/guide/rollups");
  await expect(page.locator(".docs__rail")).toBeHidden();
  await expect(page.locator(".docs__top .page-actions").first()).toBeVisible();
});

test("footer aligns to the page grid", async ({ page }) => {
  await page.goto("/skopli/guide/rollups");
  const [header, footer] = await page.evaluate(() => [
    document.querySelector(".site-header__in")!.getBoundingClientRect().left,
    document.querySelector(".site-footer__in")!.getBoundingClientRect().left,
  ]);
  expect(Math.abs(header - footer)).toBeLessThanOrEqual(1);
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
  await page.goto("/skopli/guide/getting-started");
  await page.waitForLoadState("networkidle");
  await page.waitForTimeout(500);
  expect(await page.evaluate(() => (window as unknown as { __cls: number }).__cls)).toBeLessThan(
    0.01,
  );
});

test("analytics stays off on localhost and carries the contract", async ({ page }) => {
  await page.goto("/skopli/guide/getting-started");
  await page.waitForLoadState("networkidle");
  expect(
    await page.evaluate(() => typeof (window as unknown as { posthog?: unknown }).posthog),
  ).toBe("undefined");
  const html = await (await page.request.get("/skopli/guide/getting-started")).text();
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

test("404 page renders inside the docs shell", async ({ page }) => {
  const response = await page.goto("/skopli/does-not-exist");
  expect(response?.status()).toBe(404);
  await expect(page.getByRole("heading", { level: 1 })).toHaveText("Page not found");
  await expect(page.locator(".site-header")).toBeVisible();
});

test("table of contents links land each heading under the header and mark it current", async ({
  page,
}) => {
  test.skip(!isDesktop(page), "desktop rail only");
  await page.goto("/skopli/guide/reading");
  const links = page.locator(".toc a");
  const headerBottom = await page
    .locator(".site-header")
    .evaluate((h) => h.getBoundingClientRect().bottom);
  for (let i = 0; i < (await links.count()); i++) {
    const link = links.nth(i);
    const id = (await link.getAttribute("href"))!.slice(1);
    await link.click();
    const top = await page.locator(`[id="${id}"]`).evaluate((h) => h.getBoundingClientRect().top);
    const atBottom = await page.evaluate(
      () => window.innerHeight + window.scrollY >= document.documentElement.scrollHeight - 2,
    );
    expect(top, id).toBeGreaterThan(headerBottom);
    if (!atBottom) expect(top - headerBottom, id).toBeLessThanOrEqual(32);
    await expect(link).toHaveAttribute("aria-current", "true");
  }
});

test("copying code announces the result to assistive tech", async ({ page, context }) => {
  await context.grantPermissions(["clipboard-read", "clipboard-write"]);
  await page.goto("/skopli/guide/getting-started");
  await page.locator(".code-frame [data-copy-code]").first().click({ force: true });
  const status = page.locator("#copy-status");
  await expect(status).toHaveAttribute("role", "status");
  await expect(status).toHaveText("Copied");
});
