# Discovery metadata

Both sites tell crawlers, link previews and answer engines what each page is: a `robots.txt` that names the sitemap, a sitemap on the public host, a canonical URL, Open Graph and Twitter card tags with a generated 1200x630 image, and one JSON-LD block per page.

## Sub-features

- `robots` serves `/robots.txt` at each host root with `Allow: /` and a `Sitemap:` line naming that host's `sitemap-index.xml`.
- `sitemap` serves `/sitemap-index.xml` and `/sitemap-0.xml` under each site's base; the marketing sitemap lists `https://skopli.com/`, the docs sitemap lists the 13 `https://docs.skopli.com/skopli/<slug>/` pages and nothing else.
- `canonical` gives every page one `<link rel="canonical">` with its public URL.
- `social-card` gives every page `og:title`, `og:description`, `og:type`, `og:site_name`, `og:url`, `og:image` with type, width `1200`, height `630` and alt, and a `twitter:card` of `summary_large_image` with title, description, image and alt.
- `og-image` serves the card image as `image/png`, 1200x630: `/og.png` on the marketing site and `/skopli/og/<slug>.png` for each docs page, showing the site name, page title, description and public URL in the site's own type and colours.
- `json-ld` gives every page one `<script type="application/ld+json">`: `WebSite` and `SoftwareSourceCode` on the marketing page, `TechArticle` on each docs page with `headline`, `description`, `url`, `image`, `author`, `publisher`, `isPartOf` and `about`.

## How to get to it (user POV)

- Paste a page URL into a chat or social client; the preview shows the generated card with the page title.
- Request `/robots.txt`, `/sitemap-index.xml` or `/og.png` by URL on the marketing site, and `/robots.txt`, `/skopli/sitemap-index.xml` or `/skopli/og/<slug>.png` on the docs.
- View the source of any page; the metadata sits in `<head>` before the theme script.

## Driving it with browser.mjs

Preconditions:

- Doctor reports `ok`.

```sh
node .agents/skills/verify-skopli/scripts/browser.mjs --out .verify/evidence/<run>/discovery \
  fetch http://127.0.0.1:4321/robots.txt \
  fetch http://127.0.0.1:4321/sitemap-index.xml \
  fetch http://127.0.0.1:4321/sitemap-0.xml \
  fetch http://127.0.0.1:4321/og.png \
  fetch http://127.0.0.1:4322/skopli/sitemap-index.xml \
  fetch http://127.0.0.1:4322/skopli/sitemap-0.xml \
  fetch http://127.0.0.1:4322/skopli/og/guide/reading.png \
  goto http://127.0.0.1:4321/ \
  eval "document.querySelector('link[rel=canonical]').href" \
  eval "['og:type','og:image','og:image:width','og:image:height'].map(p=>document.querySelector('meta[property=\"'+p+'\"]').content)" \
  eval "document.querySelector('meta[name=\"twitter:card\"]').content" \
  eval "JSON.parse(document.querySelector('script[type=\"application/ld+json\"]').textContent).map(o=>o['@type'])" \
  goto http://127.0.0.1:4322/skopli/guide/reading/ \
  eval "document.querySelector('link[rel=canonical]').href" \
  eval "['og:type','og:image','og:image:width','og:image:height'].map(p=>document.querySelector('meta[property=\"'+p+'\"]').content)" \
  eval "JSON.parse(document.querySelector('script[type=\"application/ld+json\"]').textContent).map(o=>o['@type']+' '+o.headline)" \
  eval "Promise.all(['index','guide/getting-started','guide/reading','guide/rollups','guide/pricing-basics','pricing/model-matching','pricing/cost-math','pricing/long-context-tiers','pricing/rollups-vs-events','reference/harnesses','reference/coverage','reference/api','contributing'].map(s=>fetch('/skopli/og/'+s+'.png').then(r=>s+':'+r.status))).then(a=>a.join(','))"
```

- **Robots.** Step 1 saves `robots.txt`: `200 text/plain`, contains `Allow: /` and `Sitemap: https://skopli.com/sitemap-index.xml`.
- **Sitemaps.** Steps 2 and 3 save the marketing sitemap: `200`, `text/xml`; `sitemap-0.xml` holds one `<loc>https://skopli.com/</loc>`. Steps 5 and 6 save the docs sitemap: `skopli_sitemap-0.xml` holds 13 `<loc>` entries, every one under `https://docs.skopli.com/skopli/` ending in `/`, none ending in `.md`, `.png` or `.txt`.
- **Card images.** Steps 4 and 7 save `og.png` and `skopli_og_guide_reading.png`: `200 image/png`. `file` on each reports `PNG image data, 1200 x 630`; opened, each shows the mark, the site name, the title, the description and the URL line on the dark paper.
- **Marketing head.** Steps 8 to 12: canonical `"https://skopli.com/"`; `["website","https://skopli.com/og.png","1200","630"]`; `"summary_large_image"`; `["WebSite","SoftwareSourceCode"]`.
- **Docs head.** Steps 13 to 16: canonical `"https://docs.skopli.com/skopli/guide/reading/"`; `["article","https://docs.skopli.com/skopli/og/guide/reading.png","1200","630"]`; `["TechArticle Reading usage"]`.
- **Every card.** Step 17 requests all 13 docs cards from the page. Every slug reports `200`.
- **Proof.** The transcript records status and content type for each fetch; the saved bodies and the two PNGs next to it are the proof.

## Gotchas

- The docs host root (`/robots.txt` and the `/` redirect) is written to `web/docs/dist/` beside `dist/skopli/`, and `astro preview` serves only `dist/skopli`, so `http://127.0.0.1:4322/robots.txt` answers `404` under launch. That is a preview limit, not a site bug. Prove the file with `cat web/docs/dist/robots.txt` (expect `Sitemap: https://docs.skopli.com/skopli/sitemap-index.xml`) or serve `web/docs/dist` with a one-off static server on a spare port.
- Every URL in the head, the sitemaps and `robots.txt` names the public host, not the local port. Fetch the local path and compare the content.
- The card images and sitemaps are build outputs; a dev server does not produce the sitemap. Verify against launch's servers.
- JSON-LD is one script with an array of objects; parse `textContent` and read `@type`, do not grep the HTML for a type name.
