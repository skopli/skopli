import type { ModelPrice } from "./types.ts";

// hand-maintained aliases for names that defeat the mechanical tiers
export const MODEL_ALIASES: Record<string, string> = {};

// families a platform rebrands with vendor decoration around the model line
// (Amazon Bedrock keys Claude as anthropic.claude-sonnet-4-5-20250929-v1:0,
// which reduces to the claude-sonnet-4.5 family key)
const ALIAS_PATTERNS: { pattern: RegExp; alias: string }[] = [
  {
    pattern: /^anthropic\.(claude-[a-z]+)-(\d+)-(\d+)(?:-\d{8})?-v\d+(?::\d+)?$/,
    alias: "$1-$2.$3",
  },
];

// vendor routing prefixes that wrap an underlying id inside a single name
// segment (Bedrock cross-region inference profiles prepend a geographic
// prefix, so us.anthropic.claude-... strips to anthropic.claude-...)
const ROUTE_PREFIXES = ["us.", "eu.", "apac.", "global."];

// provider prefixes that a catalog attaches to a bare family key (OpenRouter
// keys claude models as anthropic/claude-*, so an alias must offer that form)
const PROVIDER_PREFIXES: { pattern: RegExp; provider: string }[] = [
  { pattern: /^claude-/, provider: "anthropic" },
];

// candidate keys to try against a catalog, in decreasing confidence:
// 1. exact name
// 2. provider-prefix strip (each shorter suffix of a multi-segment path) and
//    vendor route-prefix strip (us.anthropic.claude-... -> anthropic.claude-...)
// 3. family pattern aliases (anthropic.claude-sonnet-4-5-...-v1:0 -> claude-sonnet-4.5)
// 4. separator normalization (dots vs dashes) and trailing date-stamp strip
// 5. provider-prefixed forms of a family key (claude-* -> anthropic/claude-*)
// 6. alias table
export function candidateKeys(model: string): string[] {
  const keys: string[] = [];
  const push = (key: string): void => {
    if (key !== "" && !keys.includes(key)) keys.push(key);
  };

  push(model);

  const segments = model.split("/");
  for (let i = 1; i < segments.length; i += 1) {
    push(segments.slice(i).join("/"));
  }

  const bare = segments[segments.length - 1] ?? model;
  const variants = new Set<string>([model, bare]);
  const expand = (derive: (variant: string) => string[]): void => {
    const snapshot = Array.from(variants);
    for (const variant of snapshot) {
      for (const derived of derive(variant)) variants.add(derived);
    }
  };
  expand((variant) =>
    ROUTE_PREFIXES.filter((prefix) => variant.startsWith(prefix)).map((prefix) =>
      variant.slice(prefix.length),
    ),
  );
  expand((variant) =>
    ALIAS_PATTERNS.filter(({ pattern }) => pattern.test(variant)).map(({ pattern, alias }) =>
      variant.replace(pattern, alias),
    ),
  );
  expand((variant) => [variant.replaceAll(".", "-"), variant.replace(/(\d)-(\d)/g, "$1.$2")]);
  // trailing date stamps: -20250929 or -2025-09-29
  expand((variant) => [variant.replace(/-\d{8}$/, "").replace(/-\d{4}-\d{2}-\d{2}$/, "")]);
  expand((variant) =>
    PROVIDER_PREFIXES.filter(({ pattern }) => pattern.test(variant)).map(
      ({ provider }) => `${provider}/${variant}`,
    ),
  );
  for (const variant of variants) push(variant);

  // snapshot so aliases of aliases are not chased
  const aliasCandidates = Array.from(keys);
  for (const key of aliasCandidates) {
    const alias = MODEL_ALIASES[key];
    if (alias !== undefined) push(alias);
  }

  return keys;
}

export function isZeroCost(price: ModelPrice): boolean {
  // a placeholder has every effective rate at 0: the flat rates, the 1h
  // cache-write rate, and every long-context tier's rates. A nonzero rate
  // anywhere (e.g. a paid tier atop free base rates) makes it a real price
  const flatZero =
    price.input === 0 &&
    price.output === 0 &&
    (price.cacheRead ?? 0) === 0 &&
    (price.cacheWrite ?? 0) === 0 &&
    (price.cacheWrite1h ?? 0) === 0;
  if (!flatZero) return false;
  for (const tier of price.tiers ?? []) {
    if (
      tier.input !== 0 ||
      tier.output !== 0 ||
      (tier.cacheRead ?? 0) !== 0 ||
      (tier.cacheWrite ?? 0) !== 0 ||
      (tier.cacheWrite1h ?? 0) !== 0
    ) {
      return false;
    }
  }
  return true;
}

export function matchModel(
  model: string,
  prices: ReadonlyMap<string, ModelPrice>,
  options: { skipZeroCost?: boolean } = {},
): { key: string; price: ModelPrice } | { attempted: string[]; zeroCostKey?: string } {
  const attempted = candidateKeys(model);
  for (const key of attempted) {
    const price = prices.get(key);
    if (price === undefined) continue;
    // the best match in this catalog is a placeholder: skip the whole
    // catalog rather than settle for a lower-confidence key
    if (options.skipZeroCost === true && isZeroCost(price)) {
      return { attempted, zeroCostKey: key };
    }
    return { key, price };
  }
  return { attempted };
}
