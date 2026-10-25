# Page actions

Every docs page carries one split control under its title: a `Copy page` button that copies the page as Markdown, joined to a chevron that opens a menu with the Markdown twin, a link copier and six AI handoffs.

## Sub-features

- `actions-copy` fetches the page's `.md` twin, writes it to the clipboard and flips the label to `Copied` for 1.5 seconds. The label carries `aria-live="polite"` so the flip is announced.
- `actions-menu` is the chevron button (`aria-label="More actions"`, `aria-haspopup="menu"`, `aria-expanded`). Click, Enter, Space or ArrowDown open the `role="menu"` list and the keys move focus to its first item; Escape, whether focus is on the chevron or on an item, closes it and returns focus to the chevron; Tab out or an outside click closes it. Enter or Space on an item activates it; activating an item closes the menu and returns focus to the chevron.
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
  click '[data-more]' \
  press '[data-more]' Escape \
  eval "document.querySelector('[data-more]').getAttribute('aria-expanded')+' '+document.activeElement.getAttribute('aria-label')" \
  press '[data-more]' Enter \
  press '[role=menu]' Tab \
  eval "document.querySelector('[data-more]').getAttribute('aria-expanded')+' '+document.querySelector('[role=menu]').contains(document.activeElement)" \
  click '[data-more]' \
  click 'role=menuitem[name="Copy Markdown link"]' \
  wait 'text=Link copied' \
  eval "document.querySelector('[data-more]').getAttribute('aria-expanded')+' '+document.activeElement.getAttribute('aria-label')" \
  eval "navigator.clipboard.readText()" \
  wait 'text=Copy page' \
  press '[data-more]' ArrowDown \
  press '[role=menu]' ArrowDown \
  press '[role=menu]' Enter \
  wait 'text=Link copied' \
  eval "document.querySelector('[data-more]').getAttribute('aria-expanded')+' '+document.activeElement.getAttribute('aria-label')" \
  wait 'text=Copy page' \
  press '[data-more]' ArrowDown \
  press '[role=menu]' Space \
  eval "location.pathname+' '+document.contentType" \
  goto http://127.0.0.1:4322/skopli/guide/reading/ \
  eval "[...document.querySelectorAll('footer a')].find(a=>a.textContent.trim()==='View as Markdown').getAttribute('href')"
```

The run has 39 steps; the numbers below are the `[n]` prefixes in the transcript.

- **Copy.** Steps 1 to 8 click the button and read the clipboard once the label flips. The clipboard starts with `# Reading usage` and its character count equals that of the saved `skopli_guide_reading.md` (the file is ASCII, so bytes and characters agree).
- **Label resets.** Step 9 waits for the label to read `Copy page` again after the 1.5 second reset; `wait` uses Playwright's default 30 second timeout, so a label that never resets fails the step.
- **Menu opens by keyboard.** Step 10 reads `"menu false"`. Step 11 presses ArrowDown on the chevron and step 12 reads `"true menuitem View as Markdown"`: the menu is expanded and focus sits on its first item. Step 13 saves `03-menu.png`.
- **Menu contents.** Step 14 lists eight items: `View as Markdown` with the twin href, `Copy Markdown link` with no href, then six `Open in` links whose `href` contains `Read%20https%3A%2F%2Fdocs.skopli.com%2Fskopli%2Fguide%2Freading.md`; the Cursor link uses the `cursor://` scheme.
- **Escape from an item returns focus.** Step 15 presses Escape on the focused item and step 16 reads `"false More actions"`: the menu closed and the chevron has focus again.
- **Escape from the chevron closes.** Step 17 opens the menu with a click, which leaves focus on the chevron; step 18 presses Escape there and step 19 reads `"false More actions"`.
- **Tab out closes.** Steps 20 to 22 reopen with Enter, press Tab inside the menu and read `"false false"`.
- **Copy Markdown link by mouse.** Steps 23 to 28 open the menu and click the item. Step 26 reads `"false More actions"` (the menu closed and focus is back on the chevron), step 27 reads `https://docs.skopli.com/skopli/guide/reading.md` from the clipboard and step 28 waits for the label to return to `Copy page`.
- **Copy Markdown link by keyboard.** Steps 29 to 34 open with ArrowDown, move to the second item and press Enter. Step 33 reads `"false More actions"` again: keyboard activation hands focus back to the chevron instead of dropping it on the document.
- **Space activates a link item.** Step 35 opens with ArrowDown and step 36 presses Space on `View as Markdown`; step 37 reads `"/skopli/guide/reading.md text/markdown"`, so Space followed the link.
- **Footer link.** Step 38 returns to the page and step 39 reads the footer. The href is `/skopli/guide/reading.md`.
- **Proof.** The transcript holds the clipboard excerpt and length, the focus and `aria-expanded` readings and the hrefs; `02-copied.png` shows the `Copied` label and `03-menu.png` the open menu with focus on its first item.

## Gotchas

- The copy is asynchronous. Reading the clipboard or the label straight after the click sees the old state; wait for `Copied` first.
- Do not follow the `Open in` links. They leave the local origins and `browser.mjs` blocks them; the `href` values are the proof.
- The menu is a `hidden` list, not a `<details>`; read `aria-expanded` on `[data-more]` for its state.
- A `Copy failed` label means the `.md` twin did not answer `200`; check the markdown-feeds feature before blaming the button.
- The clipboard is per browser profile, so each invocation starts empty.
