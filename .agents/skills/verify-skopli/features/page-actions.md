# Page actions

Every docs page carries three actions under its title: copy the page as Markdown to the clipboard, open the Markdown twin, and hand the twin's URL to an AI agent.

## Sub-features

- `actions-copy` fetches the page's `.md` twin, writes it to the clipboard and flips the label to `Copied` for 1.5 seconds.
- `actions-view-md` links to the page's `.md` twin.
- `actions-open-in` expands a menu of six agent links (ChatGPT, Claude, Grok, Perplexity, Google AI Mode, Cursor), each carrying a prompt that names the twin's public URL.
- `actions-footer-md` repeats `View as Markdown` in the footer.

## How to get to it (user POV)

- Open any docs page, for example `/skopli/guide/reading/`; the `Copy page`, `View as Markdown` and `Open in` controls sit under the page description.
- The footer of every docs page has a `View as Markdown` link.

## Driving it with browser.mjs

Preconditions:

- Doctor reports `ok`.

```sh
node .agents/skills/verify-skopli/scripts/browser.mjs --out .verify/evidence/<run>/page-actions \
  goto http://127.0.0.1:4322/skopli/guide/reading/ \
  screenshot 01-before \
  click 'role=button[name="Copy page"]' \
  wait 'text=Copied' \
  screenshot 02-copied \
  eval "navigator.clipboard.readText().then(t=>t.slice(0,80))" \
  eval "navigator.clipboard.readText().then(t=>t.length)" \
  fetch http://127.0.0.1:4322/skopli/guide/reading.md \
  wait 'text=Copy page' \
  eval "document.querySelector('a.actions__btn--md').getAttribute('href')" \
  click 'role=link[name="View as Markdown"] >> nth=0' \
  eval "location.pathname+' '+document.contentType" \
  goto http://127.0.0.1:4322/skopli/guide/reading/ \
  click 'text=Open in' \
  eval "[...document.querySelectorAll('.actions__menu a')].map(a=>a.textContent.trim()+' '+a.href)" \
  screenshot 03-open-in \
  click 'h1' \
  eval "document.querySelector('.actions__menu').open" \
  eval "[...document.querySelectorAll('footer a')].find(a=>a.textContent.trim()==='View as Markdown').getAttribute('href')"
```

- **Copy.** Steps 1 to 8 click the button and read the clipboard once the label flips. The clipboard starts with `# Reading usage` and its length equals the byte count of the saved `skopli_guide_reading.md`.
- **Label resets.** Step 9 waits for `Copy page` to return within two seconds.
- **View as Markdown.** Steps 10 to 12 read the href and follow the link. The href is `/skopli/guide/reading.md` and the result is `"/skopli/guide/reading.md text/markdown"`.
- **Open in.** Steps 13 to 16 go back and expand the menu. Six links, each `href` containing `Read%20https%3A%2F%2Fdocs.skopli.com%2Fskopli%2Fguide%2Freading.md`; the Cursor link uses the `cursor://` scheme.
- **Menu closes.** Steps 17 and 18 click the title. The `open` value is `false`.
- **Footer link.** Step 19 reads the footer. The href is `/skopli/guide/reading.md`.
- **Proof.** The transcript holds the clipboard excerpt and length, the hrefs and the menu state; `02-copied.png` shows the `Copied` label and `03-open-in.png` the expanded menu.

## Gotchas

- The copy is asynchronous. Reading the clipboard or the label straight after the click sees the old state; wait for `Copied` first.
- Do not follow the `Open in` links. They leave the local origins and `browser.mjs` blocks them; the `href` values are the proof.
- Narrow viewports (860px and below) hide the `View as Markdown` button under the title; the footer link remains.
- A `Copy failed` label means the `.md` twin did not answer `200`; check the markdown-feeds feature before blaming the button.
- The clipboard is per browser profile, so each invocation starts empty.
