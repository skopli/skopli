# Markdown twins and llms feeds

Every docs page has a Markdown twin at the same path with `.md` in place of the trailing slash, the docs serve an `llms.txt` index and an `llms-full.txt` with every page, and the marketing site serves its own `llms.txt`.

## Sub-features

- `md-twin` serves `/skopli/<slug>.md` as `text/markdown` with the page title, description and body for all 13 pages.
- `md-alternate` advertises the twin from each HTML page with `<link rel="alternate" type="text/markdown">`.
- `llms-index` serves `/skopli/llms.txt` listing every page's twin with its description, plus the full-file link.
- `llms-full` serves `/skopli/llms-full.txt` with all 13 pages separated by `---`.
- `llms-site` serves `/llms.txt` on the marketing site pointing at the docs feeds.

## How to get to it (user POV)

- Swap the trailing `/` of a docs URL for `.md`, for example `/skopli/guide/reading.md`.
- Follow `View as Markdown` on a docs page or in its footer.
- Follow `llms.txt` in the footer of either site; both footers lead to the docs feed, so the marketing site's own `/llms.txt` is reached by URL only.
- Request `/skopli/llms-full.txt` directly; `llms.txt` links to it as `Full docs in one file`.

## Driving it with browser.mjs

Preconditions:

- Doctor reports `ok`.

```sh
node .agents/skills/verify-skopli/scripts/browser.mjs --out .verify/evidence/<run>/markdown-feeds \
  fetch http://127.0.0.1:4322/skopli/guide/reading.md \
  fetch http://127.0.0.1:4322/skopli/guide/reading/ \
  goto http://127.0.0.1:4322/skopli/ \
  eval "Promise.all(['index','guide/getting-started','guide/reading','guide/rollups','guide/pricing-basics','pricing/model-matching','pricing/cost-math','pricing/long-context-tiers','pricing/rollups-vs-events','reference/harnesses','reference/coverage','reference/api','contributing'].map(s=>fetch('/skopli/'+s+'.md').then(r=>s+':'+r.status))).then(a=>a.join(','))" \
  fetch http://127.0.0.1:4322/skopli/llms.txt \
  fetch http://127.0.0.1:4322/skopli/llms-full.txt \
  fetch http://127.0.0.1:4321/llms.txt \
  goto http://127.0.0.1:4321/ \
  eval "[...document.querySelectorAll('footer a')].find(a=>a.textContent.trim()==='llms.txt').href"
```

- **Twin.** Steps 1 and 2 fetch a twin and the page that advertises it. The transcript shows `200 text/markdown` for the twin; `skopli_guide_reading.md` starts with `# Reading usage` and a `>` description line; `skopli_guide_reading` contains `rel="alternate" type="text/markdown" href="https://docs.skopli.com/skopli/guide/reading.md"`.
- **Every twin.** Steps 3 and 4 request all 13 twins from the page. Every slug reports `200`.
- **Index feed.** Step 5 saves `skopli_llms.txt`. It starts with `# Skopli`, has one `- [<title>](https://docs.skopli.com/skopli/<slug>.md): <description>` line per page and ends with the `Full docs in one file` link to `llms-full.txt`.
- **Full feed.** Step 6 saves `skopli_llms-full.txt` with 13 `# ` headings and 12 `---` separators.
- **Marketing feed.** Step 7 saves `llms.txt` from the marketing site: `text/plain`, starts with `# Skopli`, links the docs `llms.txt` and `llms-full.txt`.
- **Marketing footer.** Steps 8 and 9 read the footer link. Its href is `"https://docs.skopli.com/skopli/llms.txt"`, the docs feed, not the marketing file from step 7.
- **Proof.** The saved bodies next to the transcript are the proof; the transcript records status and content type for each.

## Gotchas

- The twins and feeds are build outputs. A dev server renders them too, but the content type and the pagefind-indexed HTML only match production in the built `dist`; verify against launch's servers.
- Twin links inside `llms.txt` use the public `https://docs.skopli.com` host, not the local port. Fetch the local path, compare the content.
- The docs root redirect (`/` to `/skopli/`) lives in `web/docs/dist/index.html`, which `astro preview` does not serve; `/` and `/skopli` without a slash answer `404` locally. That is a preview limit, not a site bug.
- `fetch` names the saved file after the URL path with `/` replaced by `_`; the body of `/skopli/llms.txt` is `skopli_llms.txt` and the marketing one is `llms.txt`.
