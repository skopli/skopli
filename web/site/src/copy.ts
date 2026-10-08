import { harnesses } from "@skopli/ui/data/harnesses.ts";
import { pseudolocalize, type Locale } from "@skopli/ui/i18n";

const n = harnesses.length;

export const en = {
  title: "Skopli: price your AI coding-agent usage from local logs",
  description: `An MIT SDK that reads AI coding-agent usage from ${n} harnesses on local disk, rolls it up, and prices it against market catalogs.`,
  nav: { install: "Install", layers: "Layers", quickstart: "Quickstart", harnesses: "Harnesses" },
  hero: {
    title: "Know what your coding agents cost.",
    lede: `Skopli reads the session logs ${n} coding-agent harnesses already keep on disk, rolls them up, and prices them against market catalogs.`,
    getStarted: "Read the docs",
    github: "GitHub",
    facts: "MIT. One Rust core, eight language facades, and a C ABI. No accounts, no telemetry.",
  },
  ledger: {
    caption:
      "Example output: usage rolled up by model and priced with `priceRollups`. One model has no catalog match and stays unpriced.",
    frameTitle: 'rollup(events, { by: "model" })',
    model: "Model",
    input: "Input",
    cacheRead: "Cache read",
    cacheWrite: "Cache write",
    output: "Output",
    usd: "USD",
    rates: "rates {date}",
    attempted: "tried {keys}",
    footer: "{priced} priced, {misses} unpriced",
    note: "Synthetic token counts, priced at the per-million rates in Skopli's committed catalog snapshots. Neither snapshot carries cache rates for `claude-opus-4.6`, so its cache reads and writes bill at the input rate. Totals count priced rows only.",
  },
  install: {
    title: "Install in your language",
    body: "Eight language facades and a C ABI call one Rust core. No package is published yet; they ship with the first release. Until then each SDK builds from the repository. The TypeScript package needs Node 24 or newer and ships ESM only. Java, C#, Go, and Swift link the C library and C includes its header; build it with `cargo build -p skopli-capi --release`.",
    frameTitle: "after the first release",
  },
  layers: {
    title: "Three layers. Use any one alone.",
    network: "Network",
    never: "Never",
    fetches: "Fetches, caches, reads from cache offline",
    read: {
      name: "Read",
      body: `Parse local session state from ${n} harnesses into one normalized event shape. Disk only. A file that will not parse returns a structured diagnostic.`,
    },
    rollup: {
      name: "Rollup",
      body: "Group events by model, day (timezone-aware in every SDK), session, harness, workspace, or billing block, then hand the result to pricing.",
    },
    price: {
      name: "Price",
      body: "Match models against OpenRouter, LiteLLM, and models.dev, or your own catalog, and compute USD. Long-context tiers and 5-minute and 1-hour cache writes each bill at their own rate.",
    },
  },
  quickstart: {
    title: "Four calls from local files to a priced total",
    body: "The seven handle-based facades reach the total in four calls. Rust and C expose the primitives, so their tabs show one step each. Pick a language once and both code blocks on this page follow.",
  },
  harnesses: {
    title: `Read usage from ${n} harnesses`,
    body: "Skopli parses the session state each harness keeps on disk, with no plugin, proxy, or exporter in the way. Each id is stable, and names follow the upstream project.",
    more: "Full table with footnotes",
  },
  refuses: {
    title: "What Skopli will not do",
    body: "Each of these would make a total look right while being wrong.",
    items: [
      {
        title: "Guess a price",
        body: "An unmatched model returns `priced: false` with the keys it tried, and stays out of the total.",
      },
      {
        title: "Collect telemetry",
        body: "No accounts and no telemetry. Read and Rollup never open a socket. Price fetches market catalogs when you use it, caches them, and works offline from the cache; pass an empty `sources` list or `offline` for network-free pricing.",
      },
      {
        title: "Double-count tokens",
        body: "Input, output, cache read, and cache write stay in separate buckets. Normalized output excludes reasoning tokens, and pricing bills both at the output rate. Cache writes at the 1-hour rate are counted inside the cache-write total.",
      },
      {
        title: "Hide a broken file",
        body: "The SDK never writes to stdio. Problems arrive as structured diagnostics next to your data, with the harness and path when known.",
      },
    ],
  },
  library: {
    title: "Functions your application calls",
    body: "Skopli ships read, rollup, and price as functions in your language. Call them from a dashboard, a nightly report, a CI budget check, or a billing reconciliation.",
    docs: "Documentation",
    releases: "Releases",
  },
  footer: { docs: "Docs", github: "GitHub" },
  trademark:
    "Product names and logos belong to their owners and identify the harnesses Skopli reads. Their use implies no endorsement of Skopli.",
  notFound: {
    documentTitle: "Skopli: page not found",
    title: "Page not found",
    body: "There is nothing at this address.",
    home: "Back to skopli.com",
    docs: "Read the docs",
  },
} as const;

type Widen<T> = T extends string ? string : { readonly [K in keyof T]: Widen<T[K]> };

export type Copy = Widen<typeof en>;

function pseudo(value: unknown): unknown {
  if (typeof value === "string") return pseudolocalize(value);
  if (Array.isArray(value)) return value.map(pseudo);
  return Object.fromEntries(
    Object.entries(value as Record<string, unknown>).map(([k, v]) => [k, pseudo(v)]),
  );
}

export function copyFor(locale: Locale): Copy {
  return locale === "en" ? en : (pseudo(en) as Copy);
}
