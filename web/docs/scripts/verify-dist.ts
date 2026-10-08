import { existsSync, readFileSync, readdirSync } from "node:fs";
import { join } from "node:path";
import { harnesses, unsupportedHarnesses } from "@skopli/ui/data/harnesses.ts";
import { pageActionEvents } from "@skopli/ui/analytics";
import { configuredLocales, defaultLocale } from "@skopli/ui/i18n";
import { navItems } from "../src/nav.ts";

const dist = new URL("../dist/", import.meta.url).pathname;
const site = join(dist, "skopli");
const pseudo = process.env.SKOPLI_PSEUDO_LOCALE === "1";
const locales = configuredLocales(pseudo);
const failures: string[] = [];
const need = (ok: boolean, message: string) => {
  if (!ok) failures.push(message);
};
const read = (path: string) => (existsSync(path) ? readFileSync(path, "utf8") : "");

need(
  read(join(dist, "index.html")).includes('url=/skopli/"'),
  "root index.html redirects to /skopli/",
);
need(
  read(join(dist, "robots.txt")).includes(
    "Sitemap: https://docs.skopli.com/skopli/sitemap-index.xml",
  ),
  "robots.txt names the sitemap",
);
need(existsSync(join(site, "sitemap-index.xml")), "sitemap-index.xml exists");
need(existsSync(join(site, "pagefind", "pagefind.js")), "pagefind bundle exists");
need(existsSync(join(site, "404.html")), "404.html exists");
need(existsSync(join(site, "privacy.html")), "privacy.html exists");
if (locales.length > 1) {
  need(
    readFileSync(join(site, "sitemap-0.xml"), "utf8").includes('hreflang="x-default"'),
    "sitemap carries x-default alternates",
  );
}

const slugs = ["", ...navItems.map((i) => i.slug)];
const oldNames = [
  "Copilot CLI (OTel)",
  "Kimi CLI",
  "Grok CLI",
  "fx.sh",
  "Kilo Code (VS Code)",
  "gajae-code",
  "Cost math &",
  "Rollup pricing vs",
];
const aiHosts = [
  "chatgpt.com/?q=",
  "claude.ai/new?q=",
  "grok.com/?q=",
  "perplexity.ai/search?q=",
  "google.com/search?udm=50&amp;q=",
  "cursor.com/link/prompt?text=",
];

for (const name of unsupportedHarnesses)
  for (const page of ["reference/harnesses", "reference/coverage"])
    need(read(join(site, `${page}.html`)).includes(name), `${page} names ${name} as unsupported`);

const resolves = (target: string) => {
  const path = target
    .replace(/^https:\/\/docs\.skopli\.com/, "")
    .replace(/^\/skopli\/?/, "")
    .replace(/\/$/, "");
  return (
    !path || [path, `${path}.html`, `${path}/index.html`].some((f) => existsSync(join(site, f)))
  );
};
const checkLinks = (text: string, where: string, pattern: RegExp) => {
  for (const [, target] of text.matchAll(pattern))
    need(resolves(target), `${where} links ${target}, which is not in dist`);
};
const htmlLink = /href="(\/skopli\/[^"#?]*)/g;
const mdLink = /\]\((https:\/\/docs\.skopli\.com\/skopli\/[^)#?]*)/g;

for (const locale of locales) {
  const isDefault = locale === defaultLocale;
  const prefix = isDefault ? "" : `${locale}/`;
  const llms = read(join(site, prefix, "llms.txt"));
  need(llms.startsWith("# Skopli"), `${prefix}llms.txt exists`);
  checkLinks(llms, `${prefix}llms.txt`, mdLink);
  need(
    [...llms.matchAll(mdLink)].every(([, url]) => url.includes(`/skopli/${prefix}`)),
    `${prefix}llms.txt links its own locale`,
  );
  const localeSlugs = isDefault
    ? slugs
    : slugs.filter((slug) =>
        existsSync(join(site, slug ? `${prefix}${slug}.html` : `${locale}.html`)),
      );
  need(
    read(join(site, prefix, "llms-full.txt")).split("\n---\n").length === localeSlugs.length,
    `${prefix}llms-full.txt carries one page per built page`,
  );
  if (!isDefault)
    need(
      localeSlugs.join(",") === ["", "guide/getting-started"].join(","),
      `${locale} builds exactly the fixture pages`,
    );
  for (const slug of localeSlugs) {
    const base = slug || "index";
    const htmlFile = !slug && !isDefault ? `${locale}.html` : `${prefix}${base}.html`;
    const html = read(join(site, htmlFile));
    const md = read(join(site, prefix, `${base}.md`));
    const where = `${prefix}${base}`;
    const canonical = `https://docs.skopli.com/skopli${isDefault ? `/${slug}` : `/${locale}${slug ? `/${slug}` : ""}`}`;
    need(html.length > 0, `${where}.html exists`);
    need(md.startsWith("# "), `${where}.md exists and starts with a heading`);
    need(!md.includes("@skopli/ui"), `${where}.md has no component imports`);
    need(!html.includes(":::") && !md.includes(":::"), `${where} has no literal directive fences`);
    need(html.includes(`<html lang="${locale}"`), `${where} sets lang=${locale}`);
    need(
      html.includes(`<link rel="canonical" href="${canonical}"`),
      `${where} has a canonical link`,
    );
    need(
      html.includes(`type="text/markdown" href="/skopli/${prefix}${base}.md"`),
      `${where} links its Markdown twin`,
    );
    need(
      html.includes('"@type":"WebSite"') && html.includes('"@type":"TechArticle"'),
      `${where} carries WebSite and TechArticle JSON-LD`,
    );
    need(html.includes("data-pagefind-body"), `${where} is marked for Pagefind`);
    checkLinks(html, where, htmlLink);
    checkLinks(md, `${where}.md`, mdLink);
    need(html.includes(`href="/skopli/${prefix}llms.txt"`), `${where} links its locale's llms.txt`);
    for (const host of aiHosts) need(html.includes(host), `${where} links ${host}`);
    for (const event of Object.values(pageActionEvents))
      need(html.includes(`data-event="${event}"`), `${where} carries ${event}`);
    if (isDefault)
      for (const old of oldNames)
        need(!html.includes(old) && !md.includes(old), `${where} does not mention "${old}"`);
    const translated =
      pseudo &&
      locales.every(
        (l) =>
          l === defaultLocale || existsSync(join(site, slug ? `${l}/${slug}.html` : `${l}.html`)),
      );
    if (translated)
      need(
        html.includes('hreflang="x-default"') &&
          locales.every((l) => html.includes(`hreflang="${l}"`)),
        `${where} has hreflang alternates`,
      );
    else need(!html.includes("hreflang="), `${where} has no hreflang alternates`);
  }
}

const harnessMd = read(join(site, "reference", "harnesses.md"));
for (const h of harnesses) need(harnessMd.includes(`| ${h.name}`), `harnesses.md lists ${h.name}`);
const indexed = readdirSync(site, { recursive: true, encoding: "utf8" }).filter(
  (f) => f.endsWith(".html") && read(join(site, f)).includes("data-pagefind-body"),
);
need(!read(join(site, "404.html")).includes("data-pagefind-body"), "404 is not indexed");
need(
  readdirSync(join(site, "pagefind", "fragment")).length === indexed.length,
  `pagefind indexed ${indexed.length} pages`,
);

if (failures.length) {
  console.error(`verify-dist: ${failures.length} failure(s)\n- ${failures.join("\n- ")}`);
  process.exit(1);
}
console.log(`verify-dist: ok (${locales.join(", ")})`);
