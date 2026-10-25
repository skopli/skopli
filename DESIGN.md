---
name: Skopli
description: Dark-first paper-and-ink statement aesthetic shared by skopli.com and docs.skopli.com
colors:
  paper: "#141311"
  paper-2: "#1c1b18"
  paper-3: "#26241f"
  ink: "#f1efe9"
  ink-2: "#c9c5bb"
  ink-3: "#a19d92"
  rule: "#33312c"
  rule-strong: "#6b675d"
  accent: "#f08a62"
  ok: "#6fcf97"
  miss: "#e2b45a"
  light-paper: "#ffffff"
  light-paper-2: "#f5f5f4"
  light-ink: "#141414"
  light-ink-3: "#5f5f5f"
  light-rule: "#e2e2e0"
  light-rule-strong: "#8a8a86"
  light-accent: "#b43c12"
typography:
  display:
    fontFamily: "Inter Variable, system-ui, sans-serif"
    fontSize: "clamp(2.75rem, 1.4rem + 5.2vw, 6rem)"
    fontWeight: 500
    lineHeight: 1.05
    letterSpacing: "-0.025em"
  heading:
    fontFamily: "Inter Variable, system-ui, sans-serif"
    fontSize: "1.375rem"
    fontWeight: 500
    lineHeight: 1.25
    letterSpacing: "-0.01em"
  body:
    fontFamily: "Inter Variable, system-ui, sans-serif"
    fontSize: "1rem"
    fontWeight: 400
    lineHeight: 1.6
  mono:
    fontFamily: "JetBrains Mono Variable, ui-monospace, monospace"
    fontSize: "0.875rem"
    fontWeight: 400
    lineHeight: 1.55
rounded:
  sm: "4px"
spacing:
  xs: "4px"
  sm: "8px"
  md: "16px"
  lg: "24px"
  xl: "32px"
  2xl: "48px"
  3xl: "72px"
components:
  button-primary:
    backgroundColor: "{colors.ink}"
    textColor: "{colors.paper}"
    rounded: "{rounded.sm}"
    padding: "0 24px"
    height: "44px"
  button-quiet:
    backgroundColor: "transparent"
    textColor: "{colors.ink}"
    rounded: "{rounded.sm}"
    padding: "0 24px"
    height: "44px"
  code-pane:
    backgroundColor: "{colors.paper-2}"
    textColor: "{colors.ink}"
    typography: "{typography.mono}"
    rounded: "{rounded.sm}"
    padding: "16px"
---

## Overview

Skopli presents usage and cost figures that have to be trusted, so both sites read like an audited statement: near-black paper, off-white ink, hairline rules, and numbers set in tabular figures with their source named next to them.
Marketing persuades with a real input and its real output.
Docs read as a single calm column.
Tokens live in `web/ui/src/styles/tokens.css` and are consumed unchanged by `web/site` and `web/docs`.

## Colors

Dark is the default theme, warm near-black paper with warm off-white ink.
The light theme uses neutral white and light greys for paper and a neutral near-black ink.
Three ink steps (`ink`, `ink-2`, `ink-3`) cover headings, body and muted text, all AA on paper.
`accent` is used only for links inside prose and for focus rings.
`rule` is a decorative hairline.
`rule-strong` is the 3:1 boundary used on controls such as quiet buttons and inputs.
`ok` and `miss` mark priced and unpriced states when a figure needs a status.
They are never decorative.
The light theme swaps the paper and ink scales under `[data-theme="light"]`.
The dark values live on `:root`.
The same roles apply.

## Typography

Inter Variable for all text, JetBrains Mono Variable for code, identifiers and figures.
Headings are weight 500 with slight negative tracking, in sentence case throughout.
Body measure is capped at 66ch.
Numeric columns use `font-variant-numeric: tabular-nums` and align right.

## Layout

Content is capped at 76rem on marketing and 96rem on docs (sidebar 16rem, article 44rem, table of contents 14rem).
Sections are separated by 1px rules, not by background changes or cards.
Lists of facts are definition rows: term in the left column, detail in the right, a hairline between rows.
At 390px every grid collapses to one column and tables scroll inside their own box.
Nothing overflows the page.

## Elevation & Depth

Flat.
No shadows, no gradients, no raised pills.
Hierarchy comes from type size, ink step and rules.
Sticky headers use a translucent paper background with a bottom rule.

## Shapes

One radius, 4px, on buttons, inputs, code panes and menus.
No pills.

## Components

Buttons are 44px tall.
The primary is paper text on an ink fill, the quiet variant a 1px border in `rule-strong`.
Code panes share one background (`paper-2`), a 1px rule and a small uppercase caption row.
Language tabs persist the choice in `localStorage` and switch every tab set on the page.
Page actions are one split control under the title: a quiet `Copy page` button joined to a 44px chevron that opens a flat `paper` menu with a 1px `rule-strong` border and 44px rows.
Brand marks are 16px `currentColor` SVGs in `ink-2`, vendored unmodified; a harness without a mark gets a blank 16px spacer so names stay aligned.

## Do's and Don'ts

- Do show real output from the repository's fixtures and say where the numbers come from.
- Do keep `priced: false` visible.
  A miss is a result.
- Don't invent testimonials, adoption figures or benchmarks.
- Don't add eyebrows, numbered section labels or repeated subtitles.
- Don't use the em dash in copy.
- Don't introduce a second accent or colored backgrounds for sections.
