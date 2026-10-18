#!/usr/bin/env node
import { appendFileSync, mkdirSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { chromium } from "playwright-core";

const usage = `usage: browser.mjs --out <dir> [--viewport <w>x<h>] <step> [<step> ...]
Each step is one argument naming the step followed by its arguments:
  goto <url>               navigate and wait for load
  reload                   reload the current page
  viewport <w> <h>         resize the viewport
  click <selector>         click (Playwright selector, e.g. role=button[name="Search"])
  fill <selector> <text>   fill an input
  press <selector> <key>   press a key with the element focused
  wait <selector>          wait until the element is visible
  text <selector>          record the element's text
  eval <js>                record the JSON result of a page expression (promises are awaited)
  assert <js>              fail unless the page expression is truthy
  fetch <url>              record status and content-type, save the body next to the transcript
  screenshot <name>        full-page screenshot to <out>/<name>.png
Requests to hosts other than 127.0.0.1 and localhost are blocked and logged.`;

const arity = {
  goto: 1,
  reload: 0,
  viewport: 2,
  click: 1,
  fill: 2,
  press: 2,
  wait: 1,
  text: 1,
  eval: 1,
  assert: 1,
  fetch: 1,
  screenshot: 1,
};

const argv = process.argv.slice(2);
let out;
let viewport = { width: 1280, height: 900 };
const steps = [];
for (let i = 0; i < argv.length; i++) {
  const arg = argv[i];
  if (arg === "--out") out = argv[++i];
  else if (arg === "--viewport") {
    const [w, h] = argv[++i].split("x").map(Number);
    viewport = { width: w, height: h };
  } else if (arg in arity) {
    const n = arity[arg];
    steps.push([arg, ...argv.slice(i + 1, i + 1 + n)]);
    i += n;
  } else {
    console.error(`unknown step "${arg}"\n${usage}`);
    process.exit(2);
  }
}
if (!out || steps.length === 0) {
  console.error(usage);
  process.exit(2);
}

mkdirSync(out, { recursive: true });
const transcript = join(out, "transcript.txt");
writeFileSync(transcript, "");
const log = (line) => {
  appendFileSync(transcript, `${line}\n`);
  console.log(line);
};

const local = (url) => ["127.0.0.1", "localhost"].includes(new URL(url).hostname);
const browser = await chromium.launch();
const context = await browser.newContext({ viewport });
await context.route("**/*", (route) => {
  if (local(route.request().url())) return route.continue();
  log(`blocked: ${route.request().url()}`);
  return route.abort();
});
const page = await context.newPage();
page.on("console", (m) => log(`console.${m.type()}: ${m.text()}`));
page.on("pageerror", (e) => log(`pageerror: ${e.message}`));

const run = {
  goto: async (url) => {
    await context.grantPermissions(["clipboard-read", "clipboard-write"], {
      origin: new URL(url).origin,
    });
    const response = await page.goto(url);
    return `${response?.status()} ${page.url()}`;
  },
  reload: async () => `${(await page.reload())?.status()} ${page.url()}`,
  viewport: async (w, h) => {
    await page.setViewportSize({ width: Number(w), height: Number(h) });
    return `${w}x${h}`;
  },
  click: async (sel) => page.locator(sel).first().click(),
  fill: async (sel, text) => page.locator(sel).first().fill(text),
  press: async (sel, key) => page.locator(sel).first().press(key),
  wait: async (sel) => page.locator(sel).first().waitFor({ state: "visible" }),
  text: async (sel) => (await page.locator(sel).first().innerText()).trim(),
  eval: async (js) => JSON.stringify(await page.evaluate(js)) ?? "undefined",
  assert: async (js) => {
    if (!(await page.evaluate(js))) throw new Error(`assertion failed: ${js}`);
    return "true";
  },
  fetch: async (url) => {
    if (!local(url)) throw new Error(`blocked: ${url}`);
    const response = await page.request.get(url);
    const file = new URL(url).pathname.replace(/^\/|\/$/g, "").replaceAll("/", "_") || "index";
    writeFileSync(join(out, file), await response.body());
    return `${response.status()} ${response.headers()["content-type"]} -> ${file}`;
  },
  screenshot: async (name) => {
    await page.screenshot({ path: join(out, `${name}.png`), fullPage: true });
    return `${name}.png`;
  },
};

let failed = false;
for (const [i, [name, ...args]] of steps.entries()) {
  try {
    const result = await run[name](...args);
    log(`[${i + 1}] ${name} ${args.map((a) => JSON.stringify(a)).join(" ")} -> ${result ?? "ok"}`);
  } catch (error) {
    log(
      `[${i + 1}] ${name} ${args.map((a) => JSON.stringify(a)).join(" ")} -> FAIL ${error.message}`,
    );
    await page.screenshot({ path: join(out, "failure.png"), fullPage: true }).catch(() => {});
    failed = true;
    break;
  }
}
await browser.close();
process.exit(failed ? 1 : 0);
