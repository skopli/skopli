# Product

<!-- impeccable:product-schema 1 -->

## Platform

web

## Users

Developers who run AI coding agents (Claude Code, opencode, Codex CLI, Gemini CLI, and 36 other harnesses) and want programmatic access to their own usage and cost data: dashboards, reports, billing checks, team rollups.
It is an SDK, not a CLI; users build on top of it.

## Product Purpose

An open-source SDK (MIT) that reads the session logs AI coding agents already store on local disk, normalizes them, aggregates them, and prices the token usage.
One Rust core, eight language facades (TypeScript, Python, Rust, Ruby, Java, C#, Go, Swift) plus a C ABI.
No accounts, no telemetry.
The Read and Rollup layers are fully offline and never touch the network.
The Price layer fetches market catalogs by default when used, caches what it fetches, and works offline from that cache; pass an empty `sources` list or `offline` for network-free pricing.
Success (user decision 2026-08-18): credibility and trust first.
Be the correct, reference-quality answer for agent-usage pricing; adoption follows trust.

## Positioning

An offline SDK that reads 40 harnesses' local logs through one API in 9 languages and refuses to guess prices: unmatched models return `priced: false` misses as first-class results (user-confirmed 2026-08-18).
Skopli's position is that combination: breadth across 40 harnesses and 9 languages, and explicit misses.

## Operating Context

- Users' agents already write session logs to local disk; Skopli's Read and Rollup layers read those logs in place with no instrumentation, accounts, or network calls.
  The Price layer fetches market catalogs by default when used, caches them, works offline from that cache, and never connects when given an empty `sources` list or `offline`.
- Three independent layers: read (parse logs), rollup (aggregate), price (match models to rates).
  Users can adopt any subset.
- Same API shape across languages.
  The seven handle-based facades (TypeScript, Python, Ruby, Java, C#, Go, Swift) expose the whole Price operation surface (`createPricing`, `lookupModel`, `priceRollups`, `priceEvents`, `catalogs`) and run the same built-in fetch and cache lifecycle.
  The constructor options are shared but not identical: all seven take `sources` and `fetch`, and the six non-TypeScript facades also accept explicit pre-fetched `catalogs`.
  Rust (`skopli-core`) exposes the pricing primitives directly, and the C `libskopli`/`libskopli-full` prices against injected catalogs.
  The language choice is the reader's most important docs preference and must persist.

## Capabilities and Constraints

- Explicit misses are the differentiator: when a model cannot be matched to a price, Skopli reports a miss (`priced: false` with the attempted keys) rather than guessing.
  Misses are first-class content, never hidden.
- Full pricing parity across facades: every facade exposes the same Price operation surface, and no facade ships a reduced set of operations.
  The one option difference is that the non-TypeScript facades also accept pre-fetched `catalogs`.
  The C library is network-free by default (it opens no socket); each facade fetches in its own host language and hands pre-fetched catalogs to the core, so the results match everywhere.
  Built-in fetching in the C library is a build feature (`net`), not a missing one.
- Numbers are the product: token counts, dollar totals, model names, tier boundaries.
  Numeric fidelity (tabular numerals, alignment) matters everywhere.
- Static sites, no server.
  Docs are a themed standard docs layout (sidebar nav, prev/next, search, code tabs across 9 languages).
- Pre-launch: GitHub org `skopli` and domains reserved; nothing published yet.

## Surfaces

- Marketing landing page at skopli.com.
  Mode: Persuade.
- Documentation at docs.skopli.com/skopli/.
  Mode: Read.
  13 pages: overview, guide (getting started, reading, rollups, pricing basics), pricing (model matching, cost math, long-context tiers, rollups vs events), reference (40-harness table, coverage, API), contributing.

## Hard constraints

- Both surfaces share one design system: same tokens (color, type, spacing), same visual language.
  A visitor moving from the landing page to the docs must feel zero brand seam.
- Docs must support light and dark modes; the landing page may be single-mode if the direction demands it, but its tokens must extend to a dark docs mode coherently.
- Flat design: no 3D chrome, no raised pills, no drop-shadow depth stacking (user decision 2026-08-17).
- Accessibility floor: AA contrast including muted text, focus-visible, reduced-motion honored, 44px touch targets.

## Evidence on Hand

- Passing cross-language smoke test suites and parsers exercised against real local agent logs.
  Marketing and docs may cite these (user decision 2026-08-18).
- No testimonials, no named users, no benchmarks, no adoption numbers exist.
  These must never be fabricated or implied.
- The user's own usage data is not approved as demo content.

## Product Principles

- Never guess: an explicit miss beats a plausible number, in the API, the docs, and the marketing.
- Earn trust before reach: correctness, verifiable claims, and reference-quality docs outrank growth tactics.
- Meet developers where they are: their language, their harness, their local machine, no new accounts or pipelines.
- Layers stay independent: read, rollup, and price must each be adoptable alone.

## Voice

Plain, precise, numbers-first.
States limitations.
No marketing superlatives.
Docs voice per technical-writing standards.
