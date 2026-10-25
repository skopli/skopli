# Page actions

Every docs page carries one split control under its title: a `Copy page` button that copies the page as Markdown, joined to a chevron that opens a menu with the Markdown twin, a link copier and six AI handoffs.

## Sub-features

- `actions-copy` fetches the page's `.md` twin, writes it to the clipboard and flips the label to `Copied` for 1.5 seconds. The label carries `aria-live="polite"` so the flip is announced.
- `actions-menu` is the chevron button (`aria-label="More actions"`, `aria-haspopup="menu"`, `aria-expanded`). Click, Enter, Space or ArrowDown open the `role="menu"` list and the keys move focus to its first item; Escape closes it and returns focus to the chevron; Tab out or an outside click closes it.
- `actions-view-md` is the first menu item and links to the page's `.md` twin.
- `actions-copy-link` is the second menu item and copies the twin's public URL; the label flips to `Link copied`.
- `actions-open-in` is the six remaining items (ChatGPT, Claude, Grok, Perplexity, Google AI Mode, Cursor), each carrying a prompt that names the twin's public URL.
- `actions-footer-md` repeats `View as Markdown` in the footer.

## How to get to it (user POV)

- Open any docs page, for example `/skopli/guide/reading/`; the `Copy page` control and its chevron sit under the page description.
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
  eval "document.querySelector('[data-more]').getAttribute('aria-haspopup')+' '+document.querySelector('[data-more]').getAttribute('aria-expanded')" \
  press '[data-more]' ArrowDown \
  eval "document.querySelector('[data-more]').getAttribute('aria-expanded')+' '+document.activeElement.getAttribute('role')+' '+document.activeElement.textContent.trim()" \
  screenshot 03-menu \
  eval "[...document.querySelectorAll('[role=menuitem]')].map(a=>a.textContent.trim()+' '+(a.href||''))" \
  press '[role=menu]' Escape \
  eval "document.querySelector('[data-more]').getAttribute('aria-expanded')+' '+document.activeElement.getAttribute('aria-label')" \
  press '[data-more]' Enter \
  press '[role=menu]' Tab \
  eval "document.querySelector('[data-more]').getAttribute('aria-expanded')+' '+document.querySelector('[role=menu]').contains(document.activeElement)" \
  click '[data-more]' \
  click 'role=menuitem[name="Copy Markdown link"]' \
  wait 'text=Link copied' \
  eval "navigator.clipboard.readText()" \
  wait 'text=Copy page' \
  click '[data-more]' \
  click 'role=menuitem[name="View as Markdown"]' \
  eval "location.pathname+' '+document.contentType" \
  goto http://127.0.0.1:4322/skopli/guide/reading/ \
  eval "[...document.querySelectorAll('footer a')].find(a=>a.textContent.trim()==='View as Markdown').getAttribute('href')"
```

- **Copy.** Steps 1 to 8 click the button and read the clipboard once the label flips. The clipboard starts with `# Reading usage` and its character count equals that of the saved `skopli_guide_reading.md` (the file is ASCII, so bytes and characters agree).
- **Label resets.** Step 9 waits for the label to read `Copy page` again after the 1.5 second reset; `wait` uses Playwright's default 30 second timeout, so a label that never resets fails the step.
- **Menu opens by keyboard.** Step 10 reads `"menu false"`. Step 11 presses ArrowDown on the chevron and reads `"true menuitem View as Markdown"`: the menu is expanded and focus sits on its first item.
- **Menu contents.** Step 13 lists eight items: `View as Markdown` with the twin href, `Copy Markdown link` with no href, then six `Open in` links whose `href` contains `Read%20https%3A%2F%2Fdocs.skopli.com%2Fskopli%2Fguide%2Freading.md`; the Cursor link uses the `cursor://` scheme.
- **Escape returns focus.** Step 15 reads `"false More actions"`: the menu closed and the chevron has focus again.
- **Tab out closes.** Steps 16 to 18 reopen with Enter, press Tab inside the menu and read `"false false"`.
- **Copy Markdown link.** Steps 19 to 23 copy the link; the clipboard holds `https://docs.skopli.com/skopli/guide/reading.md` and the label returns to `Copy page`.
- **View as Markdown.** Steps 24 to 26 follow the first item; the result is `"/skopli/guide/reading.md text/markdown"`.
- **Footer link.** Step 28 reads the footer. The href is `/skopli/guide/reading.md`.
- **Proof.** The transcript holds the clipboard excerpt and length, the focus and `aria-expanded` readings and the hrefs; `02-copied.png` shows the `Copied` label and `03-menu.png` the open menu with focus on its first item.

## Gotchas

- The copy is asynchronous. Reading the clipboard or the label straight after the click sees the old state; wait for `Copied` first.
- Do not follow the `Open in` links. They leave the local origins and `browser.mjs` blocks them; the `href` values are the proof.
- The menu is a `hidden` list, not a `<details>`; read `aria-expanded` on `[data-more]` for its state.
- A `Copy failed` label means the `.md` twin did not answer `200`; check the markdown-feeds feature before blaming the button.
- The clipboard is per browser profile, so each invocation starts empty.
