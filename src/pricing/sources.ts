import { loadCached, storeCached } from "./cache.ts";
import type {
  FetchLike,
  ModelPrice,
  PriceTier,
  PricingCatalog,
  PricingSource,
  SourceContext,
} from "./types.ts";

export const OPENROUTER_URL = "https://openrouter.ai/api/v1/models";
export const LITELLM_URL =
  "https://raw.githubusercontent.com/BerriAI/litellm/main/model_prices_and_context_window.json";
export const MODELS_DEV_URL = "https://models.dev/api.json";

// the on-disk cache filename a built-in source reads and writes; the single
// production formula, exposed for the constants contract test in golden/pricing/lifecycle
export function cacheFileName(name: string): string {
  return `pricing-${name}.json`;
}

// the single production description of each built-in source: name, wire-format
// tag, and default URL. The factories below derive their parser selection from
// the format tag, so this table is production data, and the constants contract
// test in golden/pricing/lifecycle compares it against constants.json
export type BuiltinFormat = "openrouter" | "litellm" | "modelsdev";

interface BuiltinDescriptor {
  name: string;
  format: BuiltinFormat;
  url: string;
}

const BUILTIN_SOURCE_DESCRIPTORS: readonly BuiltinDescriptor[] = [
  { name: "openrouter", format: "openrouter", url: OPENROUTER_URL },
  { name: "litellm", format: "litellm", url: LITELLM_URL },
  { name: "models-dev", format: "modelsdev", url: MODELS_DEV_URL },
];

export const BUILTIN_SOURCE_FORMATS: Record<string, string> = Object.fromEntries(
  BUILTIN_SOURCE_DESCRIPTORS.map((d) => [d.name, d.format]),
);

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}

function finite(value: unknown): number | null {
  if (typeof value === "number" && Number.isFinite(value)) return value;
  if (typeof value === "string" && value.trim() !== "") {
    const parsed = Number(value);
    return Number.isFinite(parsed) ? parsed : null;
  }
  return null;
}

// per-million 1h cache-write rate, or null when absent/invalid. A negative or
// overflowing above-1hr rate would bypass the input x 2 fallback with a
// nonsensical price, so it is dropped instead
function rate1h(raw: number | null): number | null {
  if (raw === null || raw < 0) return null;
  const scaled = raw * 1e6;
  return Number.isFinite(scaled) ? scaled : null;
}

async function fetchJson(fetchImpl: FetchLike, url: string): Promise<unknown> {
  const response = await fetchImpl(url);
  if (!response.ok) throw new Error(`${url} responded ${response.status}`);
  return response.json();
}

type Parser = (payload: unknown) => Map<string, ModelPrice>;

function cachedSource(name: string, url: string, parse: Parser): PricingSource {
  return {
    name,
    async load(context: SourceContext): Promise<PricingCatalog | null> {
      const cacheName = cacheFileName(name);
      const cached = loadCached(context.cacheDir, cacheName, context.ttlMs);
      // a cache payload that parses to zero prices is unusable, same as no cache
      const cachedCatalog = ((): PricingCatalog | null => {
        if (cached === null) return null;
        const prices = parse(cached.payload);
        if (prices.size === 0) return null;
        return { source: name, fetchedAt: cached.fetchedAt, prices };
      })();
      const cacheFresh = cached?.stale === false && context.refresh !== true;
      if (cachedCatalog !== null && (cacheFresh || context.offline)) {
        return cachedCatalog;
      }
      if (context.offline) return null;
      let payload: unknown;
      try {
        payload = await fetchJson(context.fetch, url);
      } catch {
        // any-age stale fallback keeps pricing available when the network is not
        return cachedCatalog;
      }
      const prices = parse(payload);
      if (prices.size === 0) return cachedCatalog;
      const fetchedAt = storeCached(context.cacheDir, cacheName, payload);
      return { source: name, fetchedAt, prices };
    },
  };
}

// OpenRouter reports USD per token as decimal strings
function parseOpenRouter(payload: unknown): Map<string, ModelPrice> {
  const prices = new Map<string, ModelPrice>();
  if (!isRecord(payload) || !Array.isArray(payload["data"])) return prices;
  for (const entry of payload["data"]) {
    if (!isRecord(entry)) continue;
    const id = entry["id"];
    const pricing = entry["pricing"];
    if (typeof id !== "string" || !isRecord(pricing)) continue;
    const input = finite(pricing["prompt"]);
    const output = finite(pricing["completion"]);
    if (input === null || output === null) continue;
    const price: ModelPrice = { input: input * 1e6, output: output * 1e6 };
    const cacheRead = finite(pricing["input_cache_read"]);
    const cacheWrite = finite(pricing["input_cache_write"]);
    if (cacheRead !== null) price.cacheRead = cacheRead * 1e6;
    if (cacheWrite !== null) price.cacheWrite = cacheWrite * 1e6;
    prices.set(id, price);
  }
  return prices;
}

const LITELLM_TIER_THRESHOLDS = [128, 200, 256, 272, 512] as const;

function parseLiteLlmTiers(entry: Record<string, unknown>): PriceTier[] {
  const tiers: PriceTier[] = [];
  for (const k of LITELLM_TIER_THRESHOLDS) {
    const suffix = `_above_${k}k_tokens`;
    const input = finite(entry[`input_cost_per_token${suffix}`]);
    const output = finite(entry[`output_cost_per_token${suffix}`]);
    const cacheRead = finite(entry[`cache_read_input_token_cost${suffix}`]);
    const cacheWrite = finite(entry[`cache_creation_input_token_cost${suffix}`]);
    const tierWrite1h = rate1h(finite(entry[`cache_creation_input_token_cost_above_1hr${suffix}`]));
    if (
      input === null &&
      output === null &&
      cacheRead === null &&
      cacheWrite === null &&
      tierWrite1h === null
    )
      continue;
    const prev = tiers.at(-1);
    const base = {
      input: prev?.input ?? (finite(entry["input_cost_per_token"]) ?? 0) * 1e6,
      output: prev?.output ?? (finite(entry["output_cost_per_token"]) ?? 0) * 1e6,
    };
    const tier: PriceTier = {
      threshold: k * 1000,
      input: input !== null ? input * 1e6 : base.input,
      output: output !== null ? output * 1e6 : base.output,
    };
    if (cacheRead !== null) tier.cacheRead = cacheRead * 1e6;
    else if (prev?.cacheRead !== undefined) tier.cacheRead = prev.cacheRead;
    if (cacheWrite !== null) tier.cacheWrite = cacheWrite * 1e6;
    else if (prev?.cacheWrite !== undefined) tier.cacheWrite = prev.cacheWrite;
    if (tierWrite1h !== null) tier.cacheWrite1h = tierWrite1h;
    tiers.push(tier);
  }
  return tiers;
}

// LiteLLM reports USD per token as numbers, keyed by model name
function parseLiteLlm(payload: unknown): Map<string, ModelPrice> {
  const prices = new Map<string, ModelPrice>();
  if (!isRecord(payload)) return prices;
  for (const [model, entry] of Object.entries(payload)) {
    if (model === "sample_spec" || !isRecord(entry)) continue;
    const input = finite(entry["input_cost_per_token"]);
    const output = finite(entry["output_cost_per_token"]);
    if (input === null || output === null) continue;
    const price: ModelPrice = { input: input * 1e6, output: output * 1e6 };
    const cacheRead = finite(entry["cache_read_input_token_cost"]);
    const cacheWrite = finite(entry["cache_creation_input_token_cost"]);
    const cacheWrite1h = finite(entry["cache_creation_input_token_cost_above_1hr"]);
    if (cacheRead !== null) price.cacheRead = cacheRead * 1e6;
    if (cacheWrite !== null) price.cacheWrite = cacheWrite * 1e6;
    const baseWrite1h = rate1h(cacheWrite1h);
    if (baseWrite1h !== null) price.cacheWrite1h = baseWrite1h;
    const tiers = parseLiteLlmTiers(entry);
    if (tiers.length > 0) {
      price.tiers = tiers;
      if (model.includes("claude")) price.tierMode = "whole-request";
    }
    prices.set(model, price);
  }
  return prices;
}

// models.dev reports USD per million tokens, nested provider -> models
function parseModelsDev(payload: unknown): Map<string, ModelPrice> {
  const prices = new Map<string, ModelPrice>();
  if (!isRecord(payload)) return prices;
  for (const [provider, providerEntry] of Object.entries(payload)) {
    if (!isRecord(providerEntry) || !isRecord(providerEntry["models"])) continue;
    for (const [model, modelEntry] of Object.entries(providerEntry["models"])) {
      if (!isRecord(modelEntry) || !isRecord(modelEntry["cost"])) continue;
      const cost = modelEntry["cost"];
      const input = finite(cost["input"]);
      const output = finite(cost["output"]);
      if (input === null || output === null) continue;
      const price: ModelPrice = { input, output };
      const cacheRead = finite(cost["cache_read"]);
      const cacheWrite = finite(cost["cache_write"]);
      if (cacheRead !== null) price.cacheRead = cacheRead;
      if (cacheWrite !== null) price.cacheWrite = cacheWrite;
      // provider-qualified key first so exact provider/model ids match, plus
      // the bare id as a lower-priority alias within the same catalog
      prices.set(`${provider}/${model}`, price);
      if (!prices.has(model)) prices.set(model, price);
    }
  }
  return prices;
}

export const FORMAT_PARSERS: Record<BuiltinFormat, Parser> = {
  openrouter: parseOpenRouter,
  litellm: parseLiteLlm,
  modelsdev: parseModelsDev,
};

function builtinSource(name: string, url?: string): PricingSource {
  const descriptor = BUILTIN_SOURCE_DESCRIPTORS.find((d) => d.name === name);
  if (descriptor === undefined) throw new Error(`unknown builtin source: ${name}`);
  return cachedSource(descriptor.name, url ?? descriptor.url, FORMAT_PARSERS[descriptor.format]);
}

export function openRouterSource(url?: string): PricingSource {
  return builtinSource("openrouter", url);
}

export function liteLlmSource(url?: string): PricingSource {
  return builtinSource("litellm", url);
}

export function modelsDevSource(url?: string): PricingSource {
  return builtinSource("models-dev", url);
}
