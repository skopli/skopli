import { createServer } from "node:http";
import { readFile } from "node:fs/promises";
import { dirname, extname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { chromium } from "playwright";

const dist = join(dirname(fileURLToPath(import.meta.url)), "..", "dist");
const out = process.argv[2] ?? "shots";
const types = {
  ".html": "text/html",
  ".css": "text/css",
  ".js": "text/javascript",
  ".svg": "image/svg+xml",
  ".woff2": "font/woff2",
};

const server = createServer(async (req, res) => {
  let path = new URL(req.url ?? "/", "http://x").pathname.slice(1);
  if (path === "" || path.endsWith("/")) path += "index.html";
  try {
    const body = await readFile(join(dist, path));
    res.writeHead(200, { "content-type": types[extname(path)] ?? "application/octet-stream" });
    res.end(body);
  } catch {
    res.writeHead(404);
    res.end();
  }
});
await new Promise((r) => server.listen(0, r));
const port = server.address().port;

const pages = ["", "404.html"];
const widths = [1440, 768, 390];
const browser = await chromium.launch();
let overflow = 0;
for (const theme of ["dark", "light"]) {
  for (const width of widths) {
    const context = await browser.newContext({
      viewport: { width, height: 900 },
      deviceScaleFactor: 1,
    });
    await context.addInitScript((t) => localStorage.setItem("skopli.theme", t), theme);
    const page = await context.newPage();
    for (const p of pages) {
      await page.goto(`http://localhost:${port}/${p}`, { waitUntil: "networkidle" });
      const wide = await page.evaluate(
        () => document.documentElement.scrollWidth > document.documentElement.clientWidth,
      );
      if (wide) {
        overflow++;
        console.log(`OVERFLOW ${width} ${theme} ${p || "index"}`);
      }
      const name = (p || "index").replace(".html", "");
      await page.screenshot({ path: join(out, `${name}-${width}-${theme}.png`), fullPage: true });
    }
    await context.close();
  }
}
await browser.close();
server.close();
console.log(`${pages.length * widths.length * 2} screenshots in ${out}; ${overflow} overflow(s)`);
process.exit(overflow ? 1 : 0);
