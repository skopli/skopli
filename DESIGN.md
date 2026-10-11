---
name: Skopli
description: Paper-and-ink statement aesthetic shared by skopli.com and docs.skopli.com
colors:
  paper: "#fbfaf7"
  paper-2: "#f3f1ec"
  paper-3: "#e9e6df"
  ink: "#16150f"
  ink-2: "#44423a"
  ink-3: "#66635a"
  rule: "#d6d2c8"
  rule-strong: "#b8b3a6"
  accent: "#b43c12"
  ok: "#176b43"
  miss: "#8a5a00"
  dark-paper: "#141311"
  dark-ink: "#f1efe9"
  dark-accent: "#f08a62"
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

Skopli presents usage and cost figures that have to be trusted, so both sites read like an audited statement: warm paper, dark ink, hairline rules, and numbers set in tabular figures with their source named next to them.
Marketing persuades with a real input and its real output; docs read as a single calm column.
Tokens live in `web/ui/src/styles/tokens.css` and are consumed unchanged by `web/site` and `web/docs`.

## Colors

Paper tones are warm off-whites, never pure white; ink is a warm near-black.
Three ink steps (`ink`, `ink-2`, `ink-3`) cover headings, body and muted text, all AA on paper.
`accent` is used only for links inside prose and for focus rings.
`ok` and `miss` mark priced and unpriced states when a figure needs a status; they are never decorative.
Dark mode swaps the paper and ink scales under `[data-theme="dark"]`; the same roles apply.

## Typography

Inter Variable for all text, JetBrains Mono Variable for code, identifiers and figures.
Headings are weight 500 with slight negative tracking; sentence case throughout.
Body measure is capped at 66ch.
Numeric columns use `font-variant-numeric: tabular-nums` and align right.

## Layout

Content is capped at 76rem on marketing and 96rem on docs (sidebar 16rem, article 44rem, table of contents 14rem).
Sections are separated by 1px rules, not by background changes or cards.
Lists of facts are definition rows: term in the left column, detail in the right, a hairline between rows.
At 390px every grid collapses to one column and tables scroll inside their own box; nothing overflows the page.

## Elevation & Depth

Flat.
No shadows, no gradients, no raised pills.
Hierarchy comes from type size, ink step and rules.
Sticky headers use a translucent paper background with a bottom rule.

## Shapes

One radius, 4px, on buttons, inputs, code panes and menus.
No pills.

## Components

Buttons are 44px tall; the primary is ink on paper, the quiet variant is a 1px inset border.
Code panes share one background (`paper-2`), a 1px rule and a small uppercase caption row.
Language tabs persist the choice in `localStorage` and switch every tab set on the page.
Page actions (copy, view as Markdown, open in an assistant) are bordered 44px controls under the title.

## Do's and Don'ts

- Do show real output from the repository's fixtures and say where the numbers come from.
- Do keep `priced: false` visible; a miss is a result.
- Don't invent testimonials, adoption figures or benchmarks.
- Don't add eyebrows, numbered section labels or repeated subtitles.
- Don't use the em dash in copy.
- Don't introduce a second accent or colored backgrounds for sections.
