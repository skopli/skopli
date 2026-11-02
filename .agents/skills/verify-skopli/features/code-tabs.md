# Code tabs

Code samples on docs pages and the marketing quickstart appear in nine language tabs; picking a language switches every tab group on the page, works from the keyboard, and is remembered on later pages of that site.

## Sub-features

- `tabs-nine` shows TypeScript, Python, Rust, Ruby, Java, C#, Go, Swift and C with TypeScript selected by default.
- `tabs-click` shows the clicked language's panel and hides the other eight.
- `tabs-keyboard` moves with ArrowRight, ArrowLeft, Home and End while a tab has focus and selects the focused tab.
- `tabs-persist` reopens later pages on the same site with the chosen language.
- `tabs-site` renders the same group in the marketing quickstart.

## How to get to it (user POV)

- Docs pages with samples: `/skopli/guide/getting-started/`, `/skopli/guide/reading/`, `/skopli/guide/rollups/`, `/skopli/guide/pricing-basics/`, `/skopli/pricing/rollups-vs-events/`.
- The marketing site at `/`, section `The same quickstart in each language`.

## Driving it with browser.mjs

Preconditions:

- Doctor reports `ok`.
- Fresh invocation, so `localStorage` holds no `skopli-lang`.

```sh
node .agents/skills/verify-skopli/scripts/browser.mjs --out .verify/evidence/<run>/code-tabs \
  goto http://127.0.0.1:4322/skopli/guide/reading/ \
  eval "[...document.querySelectorAll('[role=tab]')].map(t=>t.textContent.trim()+':'+t.getAttribute('aria-selected'))" \
  screenshot 01-default \
  click 'role=tab[name="Python"]' \
  eval "[...document.querySelectorAll('[role=tabpanel]')].map(p=>p.dataset.lang+':'+p.hidden)" \
  eval "localStorage.getItem('skopli-lang')" \
  screenshot 02-python \
  click 'role=tab[name="Go"]' \
  press 'role=tab[name="Go"]' ArrowRight \
  eval "document.activeElement.textContent.trim()+' '+localStorage.getItem('skopli-lang')" \
  goto http://127.0.0.1:4322/skopli/guide/rollups/ \
  eval "document.querySelector('[role=tab][aria-selected=true]').textContent.trim()" \
  screenshot 03-rollups-swift
```

- **Default.** Steps 1 to 3 list the tabs.
  Nine entries, `TypeScript:true` and the other eight `false`.
- **Click.** Steps 4 to 7 pick Python.
  Only `python:false` appears; the other panels are `true` (hidden); the stored value is `"python"`.
- **Keyboard.** Steps 8 to 10 focus Go and press ArrowRight.
  The result is `"Swift swift"`.
- **Persist.** Steps 11 to 13 open another page.
  The selected tab is `"Swift"`.
- **Marketing site.** Run a second invocation for the other origin.

```sh
node .agents/skills/verify-skopli/scripts/browser.mjs --out .verify/evidence/<run>/code-tabs-site \
  goto http://127.0.0.1:4321/ \
  eval "document.querySelectorAll('[role=tab]').length" \
  click 'role=tab[name="Rust"]' \
  assert "document.querySelector('[role=tabpanel][data-lang=rust]').hidden === false" \
  screenshot site-rust
```

- Nine tabs, Rust panel visible after the click.
- **Proof.** The transcripts show the selected state, hidden flags and stored value; the screenshots show the chosen tab highlighted with its code.

## Gotchas

- The language is stored per origin.
  Picking Swift on the docs does not change the marketing site; prove each on its own.
- Arrow keys only act while a tab has focus.
  Click the tab first or the keypress goes nowhere.
- A page can hold several `code-tabs` groups; selecting a language in one switches them all, so assert on every panel, not the first.
- The getting-started page renders the quickstart tabs with an install line above each sample; the tab handles are the same.
