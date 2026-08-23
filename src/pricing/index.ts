import { homedir } from "node:os";
import { join } from "node:path";
import { native } from "../native.ts";
import type { Rollup, RollupBy } from "../rollup.ts";
import type { TokenCounts, UsageEvent } from "../types.ts";
import { liteLlmSource, modelsDevSource, openRouterSource } from "./sources.ts";
import type {
  FetchLike,
  ModelPrice,
  PriceHit,
  PriceLookup,
  PriceMiss,
  PriceOverride,
  PricingCatalog,
  PricingSource,
} from "./types.ts";

export type PricingMode = "calculate" | "display" | "auto";

export type CreatePricingOptions = {
  // priority-ordered market sources; defaults to OpenRouter > LiteLLM > models.dev
  sources?: PricingSource[];
  // highest-priority prices, checked before any market source
  overrides?: PriceOverride[];
  mode?: PricingMode;
  cacheDir?: string;
  offline?: boolean;
  ttlMs?: number;
  // attempt one fresh fetch per source, bypassing fresh disk caches, with
  // stale-cache fallback if the fetch fails; later lookups on the same
  // instance use normal cache rules
  refresh?: boolean;
  fetch?: FetchLike;
};

export type PricedRollup = Rollup & {
  pricing: (PriceHit & { usd: number; tieredAggregate?: true }) | (PriceMiss & { usd?: number });
};

export type AggregatePriceHit = {
  priced: true;
  usd: number;
  models: string[];
};

export type PricedEventGroup = Rollup & {
  pricing:
    | (PriceHit & { usd: number; tieredAggregate?: true })
    | (AggregatePriceHit & { usd: number })
    | (PriceMiss & { usd?: number });
};

export type CatalogInfo = {
  source: string;
  fetchedAt: string | null;
  models: number;
};

export type PriceEventsOptions = {
  by: RollupBy;
  tz?: string;
};

export type Pricing = {
  lookupModel(model: string): Promise<PriceLookup>;
  priceTokens(model: string, tokens: TokenCounts): Promise<PriceLookup & { usd?: number }>;
  priceRollups(rollups: Rollup[]): Promise<PricedRollup[]>;
  priceEvents(events: UsageEvent[], options: PriceEventsOptions): Promise<PricedEventGroup[]>;
  // provenance of the loaded catalogs, e.g. to warn when every market source
  // failed (offline with no cache) and lookups can only miss
  catalogs(): Promise<CatalogInfo[]>;
};

export function defaultCacheDir(): string {
  const platform = process.platform;
  if (platform === "win32") {
    const local = process.env["LOCALAPPDATA"];
    if (local !== undefined && local !== "") return join(local, "skopli", "cache");
    return join(homedir(), "AppData", "Local", "skopli", "cache");
  }
  const xdg = process.env["XDG_CACHE_HOME"];
  if (xdg !== undefined && xdg !== "") return join(xdg, "skopli");
  if (platform === "darwin") return join(homedir(), "Library", "Caches", "skopli");
  return join(homedir(), ".cache", "skopli");
}

type Breakpoint = { threshold: number; rate: number };

function marginalCost(tokens: number, base: number, breakpoints: Breakpoint[]): number {
  let usd = 0;
  let prev = 0;
  let rate = base;
  for (const point of breakpoints) {
    if (tokens <= point.threshold) break;
    usd += (point.threshold - prev) * rate;
    prev = point.threshold;
    rate = point.rate;
  }
  return usd + (tokens - prev) * rate;
}

type EffectiveRates = {
  input: number;
  output: number;
  cacheRead: number;
  cacheWrite: number;
  // rate for the 1h-TTL portion of cache writes: the catalog's above-1hr rate
  // when present, else this tier's input x 2 (Anthropic's 1h multiplier)
  cacheWrite1h: number;
};
type RateStep = { threshold: number; rates: EffectiveRates };

function rateSchedule(price: ModelPrice): { base: EffectiveRates; steps: RateStep[] } {
  const tiers = [...(price.tiers ?? [])].sort((a, b) => a.threshold - b.threshold);
  let read = price.cacheRead;
  let write = price.cacheWrite;
  const base = {
    input: price.input,
    output: price.output,
    cacheRead: read ?? price.input,
    cacheWrite: write ?? price.input,
    // 1h rate is per-level: the catalog's above-1hr rate for this level, else
    // this level's input x 2. It is never inherited from a lower level, so a
    // higher tier's larger input drives a correspondingly larger 1h fallback
    cacheWrite1h: price.cacheWrite1h ?? price.input * 2,
  };
  const steps = tiers.map((tier) => {
    read = tier.cacheRead ?? read;
    write = tier.cacheWrite ?? write;
    return {
      threshold: tier.threshold,
      rates: {
        input: tier.input,
        output: tier.output,
        cacheRead: read ?? tier.input,
        cacheWrite: write ?? tier.input,
        cacheWrite1h: tier.cacheWrite1h ?? tier.input * 2,
      },
    };
  });
  return { base, steps };
}

export function costUsd(tokens: TokenCounts, price: ModelPrice): number {
  // reasoning tokens bill as output; cache tiers fall back to the input rate
  // when a catalog omits them
  const output = tokens.output + tokens.reasoning;
  const { base, steps } = rateSchedule(price);
  // tier selection context is the whole request footprint: input + cacheRead +
  // cacheWrite total (the 1h split is a billing detail within cacheWrite, not a
  // separate context bucket)
  const context = tokens.input + tokens.cacheRead + tokens.cacheWrite;
  // 1h writes bill at the tier's 1h rate; the split is clamped into
  // [0, cacheWrite] and non-finite values are treated as 0, so a malformed
  // source cannot bill more writes than the total or poison the cost
  const reported1h = tokens.cacheWrite1h;
  const finite1h = reported1h !== undefined && Number.isFinite(reported1h) ? reported1h : 0;
  const write1h = Math.min(Math.max(finite1h, 0), Math.max(tokens.cacheWrite, 0));
  const write5m = tokens.cacheWrite - write1h;
  // split-aware cache-write cost at a given rate level: the 1h portion at the
  // 1h rate, the remaining 5m portion at the base cacheWrite rate
  const cacheWriteCost = (r: EffectiveRates): number =>
    write5m * r.cacheWrite + write1h * r.cacheWrite1h;
  const bill = (r: EffectiveRates): number =>
    (tokens.input * r.input +
      output * r.output +
      tokens.cacheRead * r.cacheRead +
      cacheWriteCost(r)) /
    1_000_000;
  if (steps.length === 0) return bill(base);
  if (price.tierMode === "whole-request") {
    let active = base;
    for (const step of steps) {
      if (context <= step.threshold) break;
      active = step.rates;
    }
    return bill(active);
  }
  // marginal mode: blend input + cacheRead + cacheWrite (split-aware) into a
  // per-context-token rate so the marginal breakpoints apply to the whole
  // request footprint, then add output separately at the base rate
  const blend = (r: EffectiveRates): number =>
    context === 0
      ? 0
      : (tokens.input * r.input + tokens.cacheRead * r.cacheRead + cacheWriteCost(r)) / context;
  return (
    (marginalCost(
      context,
      blend(base),
      steps.map((s) => ({ threshold: s.threshold, rate: blend(s.rates) })),
    ) +
      output * base.output) /
    1_000_000
  );
}

export const DEFAULT_TTL_MS = 60 * 60 * 1000;
export const FETCH_TIMEOUT_MS = 10_000;

export function createPricing(options: CreatePricingOptions = {}): Pricing {
  const sources = options.sources ?? [openRouterSource(), liteLlmSource(), modelsDevSource()];
  const mode = options.mode ?? "calculate";
  const overrides = new Map<string, ModelPrice>();
  for (const override of options.overrides ?? []) {
    const { model, ...price } = override;
    overrides.set(model, price);
  }
  const baseContext = {
    // a stalled market endpoint must never hang an otherwise local run
    fetch:
      options.fetch ??
      ((url: string) => globalThis.fetch(url, { signal: AbortSignal.timeout(FETCH_TIMEOUT_MS) })),
    cacheDir: options.cacheDir ?? defaultCacheDir(),
    offline: options.offline ?? false,
    ttlMs: options.ttlMs ?? DEFAULT_TTL_MS,
  };
  // refresh is one-shot: the first load bypasses fresh disk caches, later
  // TTL-expiry reloads on the same instance use normal cache rules. Sources
  // that already completed their refresh fetch are tracked so a retry after
  // a partial failure does not refetch them a second time
  let pendingRefresh = options.refresh ?? false;
  // keyed by source object, not name: names are not required to be unique
  const refreshedSources = new Set<PricingSource>();

  // memoized only within the TTL window; afterwards sources are consulted
  // again (their own disk cache keeps that cheap when still fresh), so a
  // long-lived instance picks up refreshed market prices. Expiry is measured
  // from load completion and an in-flight load is always reused, so slow
  // sources or a tiny TTL cannot trigger overlapping duplicate loads
  let catalogsPromise: Promise<PricingCatalog[]> | null = null;
  let loadedAt: number | null = null;
  const loadCatalogs = (): Promise<PricingCatalog[]> => {
    const expired = loadedAt !== null && Date.now() - loadedAt >= baseContext.ttlMs;
    if (catalogsPromise === null || (loadedAt !== null && expired)) {
      loadedAt = null;
      const refreshing = pendingRefresh;
      pendingRefresh = false;
      const attempt = (async () => {
        const catalogs: PricingCatalog[] = [
          { source: "override", fetchedAt: null, prices: overrides },
        ];
        for (const source of sources) {
          // per-source refresh: a retry after a partial failure skips
          // sources whose refresh fetch already completed
          const refresh = refreshing && !refreshedSources.has(source);
          const catalog = await source.load({ ...baseContext, refresh });
          if (refresh) refreshedSources.add(source);
          if (catalog !== null) catalogs.push(catalog);
        }
        loadedAt = Date.now();
        return catalogs;
      })();
      // a rejected load must not be memoized, or every later call would
      // replay the same rejection with no way to retry; a rejected refresh
      // load also restores the pending refresh so the retry honours it for
      // sources that have not completed their refresh yet
      catalogsPromise = attempt;
      attempt.catch(() => {
        if (catalogsPromise === attempt) {
          catalogsPromise = null;
          if (refreshing) pendingRefresh = true;
        }
      });
    }
    return catalogsPromise;
  };

  // Serialize the TS-loaded catalogs (Map prices -> plain object, insertion
  // order preserved) into the addon's `Pricing` constructor options. The core
  // does matching + costing; source loading / cache / refresh stay here in TS.
  // `loadCatalogs` always prepends the override catalog, so its index is 0.
  //
  // The built engine is memoized against the exact catalog-load it was built
  // from: every query reuses one native `Pricing` instead of reserializing the
  // catalogs and reconstructing the addon per call. A TTL expiry or one-shot
  // refresh makes `loadCatalogs` return a new promise, which no longer matches
  // `enginePromise`'s source, so the next query rebuilds. A rejected build is
  // not memoized, mirroring `loadCatalogs`.
  let enginePromise: Promise<InstanceType<ReturnType<typeof native>["Pricing"]>> | null = null;
  let engineSource: Promise<PricingCatalog[]> | null = null;
  const buildEngine = () => {
    const catalogsPromise = loadCatalogs();
    if (enginePromise !== null && engineSource === catalogsPromise) return enginePromise;
    engineSource = catalogsPromise;
    const attempt = (async () => {
      const catalogs = await catalogsPromise;
      const optionsJson = JSON.stringify({
        mode,
        overrideIndex: 0,
        // The addon binds core directly and never fetches; source loading /
        // cache / refresh stay here in TS, so the built-in core fetch path is
        // always OFF on the wire (seam contract: no network crosses the FFI).
        builtinSources: false,
        catalogs: catalogs.map((catalog) => ({
          source: catalog.source,
          fetchedAt: catalog.fetchedAt,
          prices: Object.fromEntries(catalog.prices),
        })),
      });
      return new (native().Pricing)(optionsJson);
    })();
    enginePromise = attempt;
    attempt.catch(() => {
      if (enginePromise === attempt) {
        enginePromise = null;
        engineSource = null;
      }
    });
    return attempt;
  };

  const lookupModel = async (model: string): Promise<PriceLookup> => {
    const engine = await buildEngine();
    return JSON.parse(await engine.lookupModel(model)) as PriceLookup;
  };

  return {
    lookupModel,
    async catalogs(): Promise<CatalogInfo[]> {
      const engine = await buildEngine();
      return JSON.parse(await engine.catalogs()) as CatalogInfo[];
    },
    async priceTokens(model, tokens) {
      const lookup = await lookupModel(model);
      if (!lookup.priced) return lookup;
      return { ...lookup, usd: costUsd(tokens, lookup.price) };
    },
    async priceRollups(rollups) {
      const engine = await buildEngine();
      return JSON.parse(await engine.priceRollups(JSON.stringify(rollups))) as PricedRollup[];
    },
    async priceEvents(events, options) {
      const engine = await buildEngine();
      const optionsJson =
        options.tz === undefined
          ? JSON.stringify({ by: options.by })
          : JSON.stringify({ by: options.by, tz: options.tz });
      return JSON.parse(
        await engine.priceEvents(JSON.stringify(events), optionsJson),
      ) as PricedEventGroup[];
    },
  };
}
