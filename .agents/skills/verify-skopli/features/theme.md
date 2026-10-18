# Theme

Both sites paint dark on the first load, let the user switch to light with the header toggle, and keep the chosen theme across reloads and page changes on that site.

## Sub-features

- `theme-first-paint` renders dark before any stylesheet or script beyond the inline head script runs, so a visitor never sees a light flash.
- `theme-toggle` flips the theme on click and relabels the button for the opposite direction.
- `theme-persist` restores the chosen theme on reload and on every other page of the same site.
- `theme-site` offers the same toggle on the marketing site.

## How to get to it (user POV)

- Open any docs page, for example `/skopli/guide/reading/`, and use the sun or moon button at the right end of the header.
- Open the marketing site at `/` and use the same button in the top navigation.

## Driving it with browser.mjs

Preconditions:

- Doctor reports `ok` and the docs answer at `http://127.0.0.1:4322/skopli/`.
- Fresh invocation, so `localStorage` holds no `skopli-theme`.

```sh
node .agents/skills/verify-skopli/scripts/browser.mjs --out .verify/evidence/<run>/theme \
  goto http://127.0.0.1:4322/skopli/guide/reading/ \
  eval "document.documentElement.dataset.theme" \
  eval "localStorage.getItem('skopli-theme')" \
  fetch http://127.0.0.1:4322/skopli/guide/reading/ \
  screenshot 01-dark-first-paint \
  click 'role=button[name="Switch to light theme"]' \
  assert "document.documentElement.dataset.theme === 'light'" \
  eval "localStorage.getItem('skopli-theme')" \
  eval "document.querySelector('[data-theme-toggle]').getAttribute('aria-label')" \
  screenshot 02-light-after-toggle \
  reload \
  assert "document.documentElement.dataset.theme === 'light'" \
  eval "document.querySelector('[data-theme-toggle]').getAttribute('aria-label')" \
  screenshot 03-light-after-reload \
  click 'role=link[name="Rollups"]' \
  assert "document.documentElement.dataset.theme === 'light'" \
  click 'role=button[name="Switch to dark theme"]' \
  assert "document.documentElement.dataset.theme === 'dark'" \
  eval "localStorage.getItem('skopli-theme')"
```

- **First paint.** Steps 1 to 5 load a docs page and read the theme before touching anything. The theme is `"dark"`, the stored value is `null`, and the saved body `skopli_guide_reading` carries the `localStorage.getItem("skopli-theme")` script inside `<head>`.
- **Toggle to light.** Steps 6 to 10 click the header toggle. The page turns light, the stored value is `"light"`, and the label is now `Switch to dark theme`.
- **Persist across reload.** Steps 11 to 14 reload. The page is still light and the label still reads `Switch to dark theme`.
- **Persist across pages.** Steps 15 and 16 follow the sidebar link to `Rollups`. The next page is light too.
- **Back to dark.** Steps 17 to 19 toggle again. The theme is `"dark"` and so is the stored value.
- **Marketing site.** Run a second invocation for the other origin.

```sh
node .agents/skills/verify-skopli/scripts/browser.mjs --out .verify/evidence/<run>/theme-site \
  goto http://127.0.0.1:4321/ \
  eval "document.documentElement.dataset.theme" \
  click 'role=button[name="Switch to light theme"]' \
  assert "document.documentElement.dataset.theme === 'light'" \
  reload \
  assert "document.documentElement.dataset.theme === 'light'" \
  screenshot site-light
```

- **Proof.** The transcript shows every value above, `01-dark-first-paint.png` is dark, `02-light-after-toggle.png` and `03-light-after-reload.png` are light, and the saved page body carries the head script.

## Gotchas

- The two sites are separate origins with separate storage. A light docs site does not make the marketing site light; prove each on its own.
- The toggle's accessible name changes with the theme. `Switch to light theme` only exists while the page is dark; clicking the wrong name times out after 30 seconds.
- The theme lives on `<html data-theme>`, and `body` has a transparent background. Read the attribute or the computed colors of `document.documentElement`, not `body`.
- Full-page screenshots are tall. The header with the toggle is at the top; that is where the theme is easiest to read.
