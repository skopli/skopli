import { existsSync, readFileSync, readdirSync } from "node:fs";
import { join } from "node:path";
import { harnesses, unsupportedHarnesses } from "@skopli/ui/data/harnesses.ts";
import { pageActionEvents } from "@skopli/ui/analytics";
import { configuredLocales, defaultLocale } from "@skopli/ui/i18n";
import { navItems } from "../src/nav.ts";
import { slugsIn } from "../src/lib/translated.ts";

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
  const entries =
    readFileSync(join(site, "sitemap-0.xml"), "utf8").match(/<url>.*?<\/url>/gs) ?? [];
  const alternate = (entry: string, lang: string) =>
    entry.match(new RegExp(`hreflang="${lang}" href="([^"]+)"`))?.[1];
  const translated = entries.filter((entry) => entry.includes("hreflang="));
  need(translated.length > 0, "sitemap carries hreflang alternates");
  for (const entry of translated)
    need(
      entry.split('hreflang="x-default"').length === 2 &&
        alternate(entry, "x-default") === alternate(entry, defaultLocale),
      `sitemap entry ${entry.match(/<loc>([^<]+)/)?.[1]} points x-default at the ${defaultLocale} page`,
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

const unsupported = new Set<string>(unsupportedHarnesses);
const unsupportedLine =
  read(join(site, "reference/harnesses.html")).match(
    /no reliable local token data: ([^<]*)/,
  )?.[1] ?? "";
for (const name of unsupported)
  need(unsupportedLine.includes(name), `reference/harnesses names ${name} as unsupported`);
for (const { name } of harnesses)
  need(!unsupportedLine.includes(name), `reference/harnesses does not list ${name} as unsupported`);
const covered = [
  ...read(join(site, "reference/coverage.html")).matchAll(/<tr>\s*<td>([^<]*)<\/td>/g),
].map(([, name]) => name);
for (const name of unsupported)
  need(covered.includes(name), `reference/coverage has a row for ${name}`);
for (const h of harnesses)
  need(!covered.includes(h.name), `reference/coverage does not list the supported ${h.name}`);

const resolves = (target: string) => {
  if (!/^(https:\/\/docs\.skopli\.com)?\/skopli(\/|$)/.test(target)) return false;
  const path = target
    .replace(/^https:\/\/docs\.skopli\.com/, "")
    .replace(/^\/skopli\/?/, "")
    .replace(/\/$/, "");
  if (path.split("/").includes("..")) return false;
  return (
    !path || [path, `${path}.html`, `${path}/index.html`].some((f) => existsSync(join(site, f)))
  );
};
const checkLinks = (text: string, where: string, pattern: RegExp) => {
  for (const [, target] of text.matchAll(pattern))
    need(resolves(target), `${where} links ${target}, which is not in dist`);
};
const htmlLink = /(?:href|src)="(\/[^"#?]*)/g;
const srcset = /srcset="([^"]*)"/g;
const cssUrl = /url\(["']?(\/[^"')#?]*)/g;
const mdLink = /\]\((https:\/\/docs\.skopli\.com\/skopli\/[^)#?]*)/g;

for (const file of readdirSync(join(site, "_astro")).filter((f) => f.endsWith(".css")))
  checkLinks(read(join(site, "_astro", file)), `_astro/${file}`, cssUrl);

for (const locale of locales) {
  const isDefault = locale === defaultLocale;
  const prefix = isDefault ? "" : `${locale}/`;
  const llms = read(join(site, prefix, "llms.txt"));
  need(llms.startsWith("# Skopli"), `${prefix}llms.txt exists`);
  checkLinks(llms, `${prefix}llms.txt`, mdLink);
  const ownLocale = (url: string) =>
    isDefault
      ? locales.every((other) => other === locale || !url.includes(`/skopli/${other}/`))
      : url.includes(`/skopli/${prefix}`);
  need(
    [...llms.matchAll(mdLink)].every(([, url]) => ownLocale(url)),
    `${prefix}llms.txt links its own locale`,
  );
  const localeSlugs = isDefault
    ? slugs
    : slugs.filter((slug) =>
        existsSync(join(site, slug ? `${prefix}${slug}.html` : `${locale}.html`)),
      );
  need(
    localeSlugs.every((slug) =>
      read(join(site, prefix, "llms-full.txt")).includes(
        read(join(site, prefix, `${slug || "index"}.md`)).trimEnd(),
      ),
    ),
    `${prefix}llms-full.txt carries every built page`,
  );
  if (!isDefault)
    need(
      [...localeSlugs].sort().join(",") === slugsIn(locale).sort().join(","),
      `${locale} builds exactly its source pages`,
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
    for (const [, set] of html.matchAll(srcset))
      for (const candidate of set.split(",")) {
        const url = candidate.trim().split(/\s+/)[0] ?? "";
        if (url.startsWith("/"))
          need(resolves(url.split(/[#?]/)[0]!), `${where} srcset ${url}, which is not in dist`);
      }
    checkLinks(html, where, cssUrl);
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
