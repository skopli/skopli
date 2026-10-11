import { mkdirSync } from "node:fs";
import { chromium } from "playwright";

const [base, outDir, ...given] = process.argv.slice(2);
if (!base || !outDir) {
  console.error(
    "usage: node screens.ts <baseUrl> <outDir> [route...]  (no routes: crawl links from the base page)",
  );
  process.exit(2);
}
mkdirSync(outDir, { recursive: true });

const widths = [1440, 768, 390];
const themes = ["light", "dark"] as const;
const browser = await chromium.launch();
let failures = 0;

const crawl = async () => {
  const page = await browser.newPage();
  await page.goto(base, { waitUntil: "load" });
  const hrefs = await page.$$eval("a[href]", (as) => as.map((a) => (a as HTMLAnchorElement).href));
  await page.close();
  const inner = hrefs
    .filter((h) => h.startsWith(base) && !h.includes("#") && h.endsWith("/"))
    .map((h) => h.slice(base.length));
  return ["", ...new Set(inner)];
};
const routes = given.length ? given : await crawl();

for (const route of routes) {
  const slug = route.replace(/\/$/, "").replace(/^\//, "").replace(/\//g, "_") || "index";
  for (const width of widths) {
    for (const theme of themes) {
      const page = await browser.newPage({ viewport: { width, height: 900 }, colorScheme: theme });
      await page.addInitScript((t) => localStorage.setItem("theme", t), theme);
      await page.goto(new URL(route.replace(/^\/+/, ""), base).href, { waitUntil: "load" });
      await page.evaluate(() => document.fonts.ready);
      const overflow = await page.evaluate(() => ({
        scroll: document.documentElement.scrollWidth,
        client: document.documentElement.clientWidth,
      }));
      const file = `${outDir}/${slug}-${width}-${theme}.png`;
      await page.screenshot({ path: file, fullPage: true });
      const flag =
        overflow.scroll > overflow.client ? `OVERFLOW ${overflow.scroll}>${overflow.client}` : "ok";
      if (flag !== "ok") failures++;
      console.log(`${flag.padEnd(18)} ${file}`);
      await page.close();
    }
  }
}

await browser.close();
process.exitCode = failures ? 1 : 0;
