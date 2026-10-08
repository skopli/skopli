import { chromium, devices } from "@playwright/test";
const browser = await chromium.launch();
for (const w of [390, 768]) {
  const page = await browser.newPage({
    viewport: { width: w, height: 844 },
    isMobile: true,
    hasTouch: true,
  });
  await page.goto("http://localhost:4323/en-XA/");
  await page.waitForTimeout(300);
  const r = await page.evaluate(() => {
    const frame = document.querySelector(".ledger .table-frame");
    const scroll = frame.querySelector(".table-scroll");
    const table = frame.querySelector("table");
    const rows = [...table.querySelectorAll("tr")].map((tr) =>
      [...tr.children]
        .filter((c) => getComputedStyle(c).display !== "none")
        .map((c) => {
          const cs = getComputedStyle(c);
          return `${Math.round(c.getBoundingClientRect().width)}|${cs.whiteSpace}|${c.textContent.trim().slice(0, 28)}`;
        }),
    );
    return {
      scrollWidth: scroll.scrollWidth,
      clientWidth: scroll.clientWidth,
      tableWidth: table.getBoundingClientRect().width,
      rows,
    };
  });
  console.log(w, JSON.stringify(r, null, 1));
  await page.close();
}
await browser.close();
