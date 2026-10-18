# Skopli verification map

This directory is the maintained source for verifying the user-facing behavior of Skopli.
Read the index before driving, then use the matching feature file as the recipe.

## Baseline preconditions

- Launch with `bash .agents/skills/verify-skopli/scripts/launch.sh`.
  The marketing site answers at `http://127.0.0.1:4321/` and the docs at `http://127.0.0.1:4322/skopli/` unless `SKOPLI_SITE_PORT` or `SKOPLI_DOCS_PORT` moved them.
- `bash .agents/skills/verify-skopli/scripts/doctor.sh` prints `doctor: ok`.
- Each `browser.mjs` invocation is a fresh browser profile: no stored theme, no stored language, empty clipboard.
  A journey that needs stored state creates it inside the same invocation.
- The library recipes need `native/` and `dist/` built, which launch does.
- Never drive an instance this run did not start; launch refuses a bound port for that reason.

## Driving conventions

- Browser actions run through `node .agents/skills/verify-skopli/scripts/browser.mjs --out <dir> <steps>`.
- Library actions run through `node .agents/skills/verify-skopli/scripts/golden.mjs <harness>/<case> --out <dir>` for the golden corpus and `node .agents/skills/verify-skopli/scripts/home.mjs <home-dir> --out <dir>` for an arbitrary home directory.
- Prefer `role=` selectors with accessible names; fall back to the data attributes and classes the feature files name.
- Treat every command as literal; keep quoted names, routes and keys unchanged.
- The two sites are different origins, so `localStorage` is not shared between them; prove persistence within one origin.
- Cleanup never removes `.verify/evidence/`.

## Proof and skip reporting

- Capture the user action and the resulting state, not only the final screen.
- Browser proof is the `transcript.txt` plus screenshots of the state before and after the action.
- Fetch proof is the saved body plus the status and content type recorded in the transcript.
- Library proof is `result.json` with every match flag `true`, plus the actual outputs next to it.
- Record the feature id and the entry point used with every artifact.
- Report an unreachable path with the attempted command and the unmet precondition.
- Do not report a skipped entry point as verified through a different path.

## Feature entry contract

Each feature file starts with an H1 title and one paragraph describing the user-visible behavior.
It then uses exactly four H2 sections in this order.

1. `Sub-features` lists short ids with one line for each behavior.
2. `How to get to it (user POV)` lists every user entry point.
3. `Driving it with <harness>` starts with `Preconditions:` and uses labeled bullets that pair each user action with an exact command and observable result.
4. `Gotchas` lists traps that can waste or invalidate a run.

Keep implementation details out of the map.
Name only user paths, stable handles, required state, commands and observable proof.

## Features

- [Theme](./theme.md) covers dark first paint, the toggle on both sites, and the persisted choice across reload.
- [Docs search](./docs-search.md) covers the header search, the mobile search toggle, matching, empty and clear states.
- [Code tabs](./code-tabs.md) covers the nine-language tabs on docs pages and the marketing quickstart, keyboard navigation, and the persisted language.
- [Page actions](./page-actions.md) covers Copy page, View as Markdown and the Open in menu on every docs page.
- [Markdown twins and llms feeds](./markdown-feeds.md) covers the `.md` twin of every docs page, `llms.txt` on both sites and `llms-full.txt`.
- [Library golden replay](./library-golden.md) covers reading and rolling up a golden case through the built facade, and reading a real home directory.
