import { pseudolocalize, type Locale } from "@skopli/ui/i18n";

export const en = {
  title: "Skopli: price your AI coding-agent usage from local logs",
  description:
    "An MIT SDK that reads AI coding-agent usage from 40 harnesses on local disk, rolls it up, and prices it against market catalogs. One Rust core, nine languages.",
  nav: { install: "Install", layers: "Layers", quickstart: "Quickstart", harnesses: "Harnesses" },
  hero: {
    title: "Know what your coding agents cost.",
    lede: "Skopli reads the session logs 40 coding-agent harnesses already keep on disk, rolls them up, and prices them against market catalogs. Your code gets the total.",
    getStarted: "Read the docs",
    github: "GitHub",
    facts: "MIT. One Rust core, nine languages. No accounts, no telemetry.",
  },
  ledger: {
    caption:
      "Example output: usage rolled up by model and priced with priceRollups. One model has no catalog match and stays unpriced.",
    frameTitle: 'rollup(events, { by: "model" })',
    model: "Model",
    input: "Input",
    cacheRead: "Cache read",
    cacheWrite: "Cache write",
    output: "Output",
    usd: "USD",
    rates: "rates {date}",
    attempted: "tried {keys}",
    footer: "{priced} priced, {misses} miss",
    note: "Synthetic token counts, priced at the per-million rates in Skopli's committed catalog snapshots ({date}). The miss stays out of the total: Skopli returns priced: false with the keys it tried, never a guessed rate.",
  },
  install: {
    title: "Install in your language",
    body: "Nine facades call one Rust core. The TypeScript package needs Node 24 or newer and ships ESM only. The rest publish with the first release.",
  },
  layers: {
    title: "Three layers. Use any one alone.",
    read: {
      name: "Read",
      body: "Parse local session state from 40 harnesses into one normalized event shape. Disk only. A file that will not parse returns a structured diagnostic, not a silent gap.",
    },
    rollup: {
      name: "Rollup",
      body: "Group events by model, day (timezone-aware in every SDK), session, harness, workspace, or billing block, then hand the result to pricing.",
    },
    price: {
      name: "Price",
      body: "Match models against OpenRouter, LiteLLM, and models.dev, or your own catalog, and compute USD. Long-context tiers and 5-minute and 1-hour cache writes each bill at their own rate. Catalogs are fetched by default, cached, and read from cache offline.",
    },
  },
  quickstart: {
    title: "Four calls from local files to a priced total",
    body: "The same calls run in every facade. Pick a language once; the choice follows you into the docs.",
  },
  harnesses: {
    title: "40 harnesses, read where they already write",
    body: "No plugin, proxy, or exporter. Skopli parses the session state each harness keeps on disk. Ids are stable; names follow the upstream project.",
  },
  refuses: {
    title: "What Skopli will not do",
    body: "A wrong number is worse than a gap you can see.",
    items: [
      {
        title: "Guess a price",
        body: "An unmatched model returns priced: false with the keys it tried, and stays out of the total.",
      },
      {
        title: "Phone home",
        body: "No accounts and no telemetry. Read and Rollup never open a socket. Price fetches market catalogs when you use it, caches them, and works offline from the cache; pass an empty sources list or offline for network-free pricing.",
      },
      {
        title: "Double-count tokens",
        body: "Input, output, cache read, and cache write stay in separate buckets. Reasoning tokens are subtracted from output at the source. 1-hour cache writes are a portion of the cache-write total, not an addition to it.",
      },
      {
        title: "Hide a broken file",
        body: "The SDK never writes to stdio. Problems arrive as structured diagnostics next to your data, with the harness and path when known.",
      },
    ],
  },
  library: {
    title: "A library, not a service",
    body: "Skopli ships read, rollup, and price as functions, so what you build on top is yours: a dashboard, a nightly report, a CI budget check, a billing reconciliation.",
    docs: "Documentation",
    releases: "Releases",
  },
  footer: { docs: "Docs", github: "GitHub", license: "MIT License" },
  trademark:
    "Product names and logos belong to their owners and identify the harness each reader supports. Their use implies no endorsement of Skopli.",
  notFound: {
    title: "Page not found",
    body: "There is nothing at this address. The docs live at docs.skopli.com.",
    home: "Back to skopli.com",
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
