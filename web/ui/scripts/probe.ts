import { chromium, type Browser, type BrowserContextOptions } from "playwright";

const failures: string[] = [];

export const check = (name: string, ok: boolean, detail = "") => {
  console.log(`${ok ? "ok  " : "FAIL"} ${name}${detail ? ` (${detail})` : ""}`);
  if (!ok) failures.push(name);
};

export const launch = async (options: BrowserContextOptions = {}) => {
  const browser = await chromium.launch();
  const page = await browser.newPage({ viewport: { width: 1440, height: 900 }, ...options });
  return { browser, page };
};

export const finish = async (browser: Browser) => {
  await browser.close();
  console.log(failures.length ? `\n${failures.length} failed` : "\nall passed");
  process.exit(failures.length ? 1 : 0);
};
