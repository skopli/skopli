import { mkdir } from "node:fs/promises";
import { chromium } from "@playwright/test";
import { serveDist } from "./serve-dist.ts";

export interface ScreenPage {
  name: string;
  path: string;
}

export interface ScreenOptions {
  pages: ScreenPage[];
  dist: string;
  notFound: string;
  outDir: string;
  port: number;
}

const viewports = {
  "1440x900": [1440, 900],
  "768x1024": [768, 1024],
  "390x844": [390, 844],
} as const;
const themes = ["dark", "light"] as const;

export async function captureScreens({ pages, dist, notFound, outDir, port }: ScreenOptions) {
  await mkdir(outDir, { recursive: true });
  const server = await serveDist(dist, port, notFound);
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
        if (scrollWidth > width)
          overflow.push(`${target.name} ${label} ${theme}: ${scrollWidth}px`);
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
}
