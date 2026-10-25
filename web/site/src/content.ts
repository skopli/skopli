export const title = "Skopli: usage and cost data from 40 coding agents, read offline";
export const description =
  "An MIT SDK that reads the session logs AI coding agents already keep on disk, normalizes them, rolls them up and prices them against market catalogs. One Rust core, 9 languages, no accounts, no telemetry.";

export const layers = [
  {
    name: "Read",
    does: "Parses the session state 40 harnesses keep on disk into one event shape: harness, session, message, turn, subagent flag, model, token buckets. Malformed rows become diagnostics, not crashes.",
    network: "Never",
  },
  {
    name: "Rollup",
    does: "Groups events by model, day, session, harness, workspace or billing block. Day bucketing takes an IANA timezone and matches it in every SDK.",
    network: "Never",
  },
  {
    name: "Price",
    does: "Looks each model up in OpenRouter, LiteLLM and models.dev, caches the catalogs for an hour and keeps serving the cache when the network is gone. Pass <code>offline</code> or an empty source list and it never connects.",
    network: "Cached, optional",
  },
];

export const rules = [
  ["Reasoning tokens", "billed at the output rate"],
  ["Cache read and write", "fall back to the input rate when a catalog omits them"],
  ["1h cache writes", "billed at the catalog's 1h rate when it has one, else input x 2"],
  [
    "Long-context tiers",
    "128k, 200k, 256k, 272k and 512k thresholds, marginal by default and whole-request for Claude models",
  ],
  ["Zero-token rows", "dropped, whatever cost the vendor reported"],
  ["Unmatched models", "<code>priced: false</code> with the keys that were tried, never a guess"],
];
