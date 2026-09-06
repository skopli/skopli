// Loader + typed surface for the native `skopli-node` addon (napi-rs). The
// addon is built by `pnpm run build:native` into the repo-root `native/`
// directory (`native/index.cjs` + the platform `.node`). Both this module's
// source location (`src/`) and its compiled location (`dist/`) sit one level
// under the repo root, so the addon resolves at `../native/index.cjs` from either.
//
// The binding is emitted as `.cjs` (not `.js`) so Node treats it as CommonJS
// even though this package is `"type": "module"`, which lets `createRequire`
// load it synchronously - the sync facade calls (rollup/costUsd/...) need that.
//
// The addon is loaded via `createRequire` against a path computed from
// `import.meta.url` rather than a static `import`, so the TypeScript build
// (rootDir: "src") never tries to typecheck or emit the out-of-root addon files.
// The `NativeAddon` interface below is the hand-checked mirror of the generated
// `native/index.d.ts`; the whole boundary is JSON strings in and out.

import { createRequire } from "node:module";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

export type Pricing = {
  catalogs(): Promise<string>;
  lookupModel(model: string): Promise<string>;
  priceRollups(rollupsJson: string): Promise<string>;
  priceEvents(eventsJson: string, optionsJson: string): Promise<string>;
};

export type PricingConstructor = new (optionsJson: string) => Pricing;

export type NativeAddon = {
  supportedHarnesses(): string[];
  defaultCacheDir(): string;
  rollup(eventsJson: string, optionsJson: string): string;
  costUsd(tokensJson: string, priceJson: string): number;
  readHarness(harness: string, optionsJson: string): Promise<string>;
  Pricing: PricingConstructor;
};

function loadAddon(): NativeAddon {
  const here = dirname(fileURLToPath(import.meta.url));
  const entry = join(here, "..", "native", "index.cjs");
  const require = createRequire(import.meta.url);
  try {
    return require(entry) as NativeAddon;
  } catch (cause) {
    throw new Error(
      `skopli native addon not found at ${entry}. Build it with \`pnpm run build:native\` (requires a Rust toolchain).`,
      { cause },
    );
  }
}

let cached: NativeAddon | null = null;

// Lazy so importing a facade module for a type alone never forces the addon to
// load - only an actual call does.
export function native(): NativeAddon {
  if (cached === null) cached = loadAddon();
  return cached;
}
