import { mkdir } from "node:fs/promises";
import { chromium } from "@playwright/test";
import { navItems } from "../src/nav.ts";
import { serveDist } from "./serve-dist.ts";

const viewports = {
  "1440x900": [1440, 900],
  "768x1024": [768, 1024],
  "390x844": [390, 844],
} as const;
const themes = ["dark", "light"] as const;
const port = 4399;
const outDir = new URL("../../../artifacts/screens/docs/", import.meta.url).pathname;
const only = process.argv.slice(2);

const pages = [
  { name: "index", path: "/skopli/" },
  ...navItems.map((item) => ({ name: item.slug.replace("/", "-"), path: `/skopli/${item.slug}` })),
  { name: "404", path: "/skopli/this-page-does-not-exist" },
  ...(process.env.SKOPLI_PSEUDO_LOCALE === "1"
    ? [
        { name: "en-XA-index", path: "/skopli/en-XA" },
        { name: "en-XA-guide-getting-started", path: "/skopli/en-XA/guide/getting-started" },
      ]
    : []),
].filter((page) => !only.length || only.includes(page.name));

await mkdir(outDir, { recursive: true });
const server = await serveDist(new URL("../dist", import.meta.url).pathname, port);
const browser = await chromium.launch();
const overflow: string[] = [];
for (const [label, [width, height]] of Object.entries(viewports)) {
  for (const theme of themes) {
    const context = await browser.newContext({
      viewport: { width, height },
      deviceScaleFactor: 1,
      hasTouch: width < 800,
    });
    await context.addInitScript((value) => localStorage.setItem("skopli-theme", value), theme);
    const page = await context.newPage();
    for (const target of pages) {
      await page.goto(`http://localhost:${port}${target.path}`, { waitUntil: "networkidle" });
      await page.evaluate(() => document.fonts.ready);
      const scrollWidth = await page.evaluate(() => document.documentElement.scrollWidth);
      if (scrollWidth > width) overflow.push(`${target.name} ${label} ${theme}: ${scrollWidth}px`);
      await page.screenshot({
        path: `${outDir}${target.name}--${label}--${theme}.png`,
        fullPage: true,
      });
    }
    await context.close();
  }
}
await browser.close();
server.close();
if (overflow.length) {
  console.error(`horizontal overflow:\n${overflow.join("\n")}`);
  process.exit(1);
}
console.log(`screens: ${pages.length * 6} written to ${outDir}`);
