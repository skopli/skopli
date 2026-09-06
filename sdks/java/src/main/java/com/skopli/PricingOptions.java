package com.skopli;

import java.util.ArrayList;
import java.util.List;
import java.util.Optional;
import java.util.OptionalLong;

/**
 * Options for {@link Skopli#createPricing}, at parity with the TypeScript
 * {@code CreatePricingOptions}. {@link PricingSource}s are resolved facade-side
 * (each {@code load} runs in Java, fetching and caching in the host language)
 * and their catalogs are merged with any explicit {@code catalogs} and
 * {@code overrides} before being handed to the core. The C ABI performs no
 * network I/O, so {@code builtinSources} is always sent {@code false}; the seam
 * lives entirely here.
 *
 * <p>Priority highest-first: {@code overrides}, then {@code catalogs}, then the
 * resolved {@code sources} (default OpenRouter, LiteLLM, models.dev). A
 * {@code null} sources list defaults to the built-ins; an empty list
 * contributes none. {@code cacheDir} empty resolves to the platform default via
 * {@code ag_default_cache_dir}; {@code ttlMs} empty defaults to 1h.
 */
public record PricingOptions(
        PricingMode mode,
        List<PriceOverride> overrides,
        List<Catalog> catalogs,
        List<PricingSource> sources,
        String cacheDir,
        boolean offline,
        OptionalLong ttlMs,
        boolean refresh,
        Optional<Fetcher> fetch,
        Optional<Clock> clock) {

    /** The default cache freshness window (1 hour). */
    public static final long DEFAULT_TTL_MS = 60 * 60 * 1000;

    public static Builder builder() {
        return new Builder();
    }

    /** Builder for {@link PricingOptions}. */
    public static final class Builder {
        private PricingMode mode = PricingMode.CALCULATE;
        private final List<PriceOverride> overrides = new ArrayList<>();
        private final List<Catalog> catalogs = new ArrayList<>();
        private List<PricingSource> sources;
        private String cacheDir;
        private boolean offline;
        private Long ttlMs;
        private boolean refresh;
        private Fetcher fetch;
        private Clock clock;

        public Builder mode(PricingMode mode) {
            this.mode = mode;
            return this;
        }

        public Builder override(PriceOverride override) {
            this.overrides.add(override);
            return this;
        }

        public Builder catalog(Catalog catalog) {
            this.catalogs.add(catalog);
            return this;
        }

        public Builder source(PricingSource source) {
            if (this.sources == null) {
                this.sources = new ArrayList<>();
            }
            this.sources.add(source);
            return this;
        }

        public Builder sources(List<PricingSource> sources) {
            this.sources = sources == null ? null : new ArrayList<>(sources);
            return this;
        }

        public Builder cacheDir(String cacheDir) {
            this.cacheDir = cacheDir;
            return this;
        }

        public Builder offline(boolean offline) {
            this.offline = offline;
            return this;
        }

        public Builder ttlMs(long ttlMs) {
            this.ttlMs = ttlMs;
            return this;
        }

        public Builder refresh(boolean refresh) {
            this.refresh = refresh;
            return this;
        }

        public Builder fetch(Fetcher fetch) {
            this.fetch = fetch;
            return this;
        }

        public Builder clock(Clock clock) {
            this.clock = clock;
            return this;
        }

        public PricingOptions build() {
            return new PricingOptions(
                    mode,
                    List.copyOf(overrides),
                    List.copyOf(catalogs),
                    sources == null ? null : List.copyOf(sources),
                    cacheDir == null ? "" : cacheDir,
                    offline,
                    ttlMs == null ? OptionalLong.empty() : OptionalLong.of(ttlMs),
                    refresh,
                    Optional.ofNullable(fetch),
                    Optional.ofNullable(clock));
        }
    }
}
