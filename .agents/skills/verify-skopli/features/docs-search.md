# Docs search

Search lets a reader find docs pages and sections by typing into the header box, open a result, and tell no matches apart from a search that never loaded.

## Sub-features

- `search-open` shows the search box in the header on wide screens and behind the magnifier toggle on narrow ones.
- `search-match` lists matching pages with their sections while the reader types.
- `search-open-result` navigates to the chosen page or section.
- `search-empty` shows `No results for <query>` for a query that matches nothing.
- `search-clear` empties the box and removes the results.

## How to get to it (user POV)

- On any docs page at a width above 860px, click into the `Search docs` box in the header.
- At 860px or below, press the magnifier button labeled `Search`, which reveals the box and focuses it.

## Driving it with browser.mjs

Preconditions:

- Doctor reports `ok`, which includes `/skopli/pagefind/pagefind-ui.js` answering; search exists only in the built output.

```sh
node .agents/skills/verify-skopli/scripts/browser.mjs --out .verify/evidence/<run>/docs-search \
  goto http://127.0.0.1:4322/skopli/guide/reading/ \
  wait 'role=textbox[name="Search docs"]' \
  fill 'role=textbox[name="Search docs"]' timezone \
  wait '.pagefind-ui__result' \
  text '.pagefind-ui__message' \
  eval "[...document.querySelectorAll('.pagefind-ui__result-link')].map(a=>a.textContent.trim()+' '+a.getAttribute('href'))" \
  screenshot 01-results \
  click 'role=link[name="Rollups"] >> nth=0' \
  eval "location.pathname" \
  wait 'role=textbox[name="Search docs"]' \
  fill 'role=textbox[name="Search docs"]' zzqxv \
  wait 'text=/zzqxv/' \
  text '.pagefind-ui__message' \
  screenshot 02-empty \
  click '.pagefind-ui__search-clear' \
  eval "document.querySelector('.pagefind-ui__search-input').value" \
  eval "document.querySelectorAll('.pagefind-ui__result').length"
```

- **Wide entry.** Steps 1 and 2 load the page and wait for the box with placeholder `Search docs`.
- **Match.** Steps 3 to 7 type `timezone`. The message reads `3 results for timezone` and the links include `Rollups /skopli/guide/rollups/` and `Coverage /skopli/reference/coverage/` with their section hits.
- **Open result.** Steps 8 and 9 follow the first `Rollups` link. The path is `/skopli/guide/rollups/`.
- **Empty state.** Steps 10 to 14 search `zzqxv` on the new page. The message reads `No results for zzqxv`.
- **Clear.** Steps 15 to 17 press the clear button. The value is `""` and the result count is `0`.
- **Narrow entry.** Run a second invocation at phone width.

```sh
node .agents/skills/verify-skopli/scripts/browser.mjs --out .verify/evidence/<run>/docs-search-mobile --viewport 390x844 \
  goto http://127.0.0.1:4322/skopli/guide/reading/ \
  eval "getComputedStyle(document.querySelector('[data-search]')).display" \
  click 'role=button[name="Search"]' \
  eval "document.querySelector('[data-search-toggle]').getAttribute('aria-expanded')" \
  eval "document.activeElement.className" \
  screenshot 03-mobile-open
```

- The display is `"none"` before the click, `aria-expanded` is `"true"` after it, and the active element has class `pagefind-ui__search-input`.
- **Proof.** The transcripts hold the result list, the empty message and the toggle state; `01-results.png` shows the drawer under the header and `03-mobile-open.png` the revealed box.

## Gotchas

- Results update after a short debounce. Wait for `.pagefind-ui__result` or for text containing the new query, never a fixed sleep; reading the message right after `fill` returns the previous query's message.
- The input has no ARIA label; its accessible name is the placeholder `Search docs`, so `role=textbox[name="Search docs"]` is the handle and `role=searchbox` does not match.
- Result links repeat page titles for section hits. Use `>> nth=0` or the `href` when a title appears more than once.
- A dev server (`astro dev`) has no pagefind bundle, so the box never appears there. Verify against the built output launch serves.
- The `Search` toggle and `Menu` button only exist in the DOM as visible controls at 860px and below.
