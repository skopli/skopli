import { existsSync, readFileSync } from "node:fs";
import { join } from "node:path";
import { harnesses } from "@skopli/ui/data/harnesses.ts";
import { configuredLocales, defaultLocale } from "@skopli/ui/i18n";
import { rows, totals, usd } from "../src/data/example.ts";

const dist = new URL("../dist/", import.meta.url).pathname;
const locales = configuredLocales(process.env.SKOPLI_PSEUDO_LOCALE === "1");
const failures: string[] = [];
const need = (ok: boolean, message: string) => {
  if (!ok) failures.push(message);
};
const read = (path: string) => (existsSync(path) ? readFileSync(path, "utf8") : "");

need(
  read(join(dist, "robots.txt")).includes("Sitemap: https://skopli.com/sitemap-index.xml"),
  "robots.txt names the sitemap",
);
const sitemap = read(join(dist, "sitemap-0.xml"));
need(
  [...sitemap.matchAll(/<loc>([^<]*)<\/loc>/g)].every(([, loc]) => !/\/404(\.html)?\/?$/.test(loc)),
  "sitemap omits the 404 page",
);
if (locales.length > 1) need(sitemap.includes('hreflang="x-default"'), "sitemap has x-default");
need(existsSync(join(dist, "favicon.svg")), "favicon exists");
need(existsSync(join(dist, "og.png")), "og image exists");

const llms = read(join(dist, "llms.txt"));
need(llms.startsWith("# Skopli"), "llms.txt has the title");
need(
  llms.includes(`## Supported harnesses (${harnesses.length})`),
  "llms.txt counts the harnesses",
);
for (const h of harnesses) need(llms.includes(`- ${h.name} (${h.id})`), `llms.txt lists ${h.name}`);

const notFound = read(join(dist, "404.html"));
need(notFound.includes('<meta name="robots" content="noindex"'), "404 is noindex");
need(notFound.includes("Page not found"), "404 renders the copy");
need(
  !/rel="canonical"|property="og:url"|hreflang=/.test(notFound),
  "404 has no canonical, og:url, or hreflang",
);

for (const locale of locales) {
  const isDefault = locale === defaultLocale;
  const html = read(join(dist, isDefault ? "index.html" : `${locale}/index.html`));
  const label = isDefault ? "index" : `${locale}/index`;
  const url = `https://skopli.com/${isDefault ? "" : `${locale}/`}`;
  need(html.length > 0, `${label} exists`);
  need(sitemap.includes(`<loc>${url}</loc>`), `sitemap lists ${label}`);
  need(html.includes(`<html lang="${locale}"`), `${label} has lang`);
  need(html.includes(`<link rel="canonical" href="${url}"`), `${label} has a canonical link`);
  need(html.includes('"@type":"SoftwareSourceCode"'), `${label} has SoftwareSourceCode JSON-LD`);
  need(
    html.includes('"codeRepository":"https://github.com/skopli/skopli"'),
    `${label} names the repository`,
  );
  need(html.includes('<meta property="og:title"'), `${label} has Open Graph tags`);
  need(
    html.includes('<meta property="og:image" content="https://skopli.com/og.png"'),
    `${label} has an og:image`,
  );
  need(html.includes("https://eu.i.posthog.com"), `${label} loads analytics`);
  need(html.includes('href="https://docs.skopli.com/skopli/"'), `${label} links the docs`);
  for (const h of harnesses) need(html.includes(`>${h.name}</`), `${label} lists ${h.name}`);
  for (const row of rows) need(html.includes(row.model), `${label} ledger has ${row.model}`);
  need(html.includes(usd(totals.usd)), `${label} ledger shows the total ${usd(totals.usd)}`);
  need(html.includes('class="ledger__miss"'), `${label} ledger shows the miss`);
  if (locales.length > 1) {
    need(html.includes(`hreflang="${locale}"`), `${label} has hreflang`);
    need(html.includes('hreflang="x-default"'), `${label} has x-default hreflang`);
  }
}

if (failures.length) {
  console.error(`verify-dist: ${failures.length} failure(s)\n- ${failures.join("\n- ")}`);
  process.exit(1);
}
console.log(`verify-dist: ok (${locales.join(", ")})`);
