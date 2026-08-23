// all prices are USD per million tokens
export type PriceTier = {
  threshold: number;
  input: number;
  output: number;
  cacheRead?: number;
  cacheWrite?: number;
  // rate for 1h-TTL cache writes at this tier; when omitted, falls back to this
  // tier's own input x 2 (never inherited from a lower level)
  cacheWrite1h?: number;
};

export type ModelPrice = {
  input: number;
  output: number;
  cacheRead?: number;
  cacheWrite?: number;
  // rate for 1h-TTL cache writes (LiteLLM cache_creation_input_token_cost_above_1hr)
  cacheWrite1h?: number;
  tiers?: PriceTier[];
  tierMode?: "marginal" | "whole-request";
};

export type PriceOverride = ModelPrice & {
  model: string;
};

export type PricingCatalog = {
  source: string;
  // null for programmatic overrides, which have no fetch time
  fetchedAt: string | null;
  prices: Map<string, ModelPrice>;
};

export type FetchLike = (url: string) => Promise<{
  ok: boolean;
  status: number;
  json(): Promise<unknown>;
}>;

export type SourceContext = {
  fetch: FetchLike;
  cacheDir: string;
  offline: boolean;
  ttlMs: number;
  // treat fresh disk-cached catalogs as stale, forcing a refetch (stale
  // fallback still applies when the refetch fails)
  refresh?: boolean;
};

export type PricingSource = {
  name: string;
  load(context: SourceContext): Promise<PricingCatalog | null>;
};

export type PriceHit = {
  priced: true;
  model: string;
  // catalog key that matched, which may differ from the requested model name
  key: string;
  price: ModelPrice;
  source: string;
  fetchedAt: string | null;
};

export type PriceMiss = {
  priced: false;
  model: string;
  attempted: string[];
} & ({ reason?: never; key?: never } | { reason: "zeroCost"; key: string });

export type PriceLookup = PriceHit | PriceMiss;
