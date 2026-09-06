package com.skopli;

import java.util.Optional;

/**
 * Context passed to {@link PricingSource#load}. Carries the injectable
 * {@link Fetcher} (empty means the source must not perform network I/O) and the
 * resolved cache lifecycle knobs: the {@link Clock} seam, the on-disk
 * {@code cacheDir}, {@code offline} (serve cache only, skip a source with no
 * cache), {@code ttlMs} (freshness window, {@code age > ttlMs} is stale), and
 * {@code refresh} (force one fresh fetch, bypassing a fresh disk cache, with a
 * stale-cache fallback if the fetch fails).
 */
public record SourceContext(
        Optional<Fetcher> fetch,
        Clock clock,
        String cacheDir,
        boolean offline,
        long ttlMs,
        boolean refresh) {

    public static SourceContext of(Fetcher fetch) {
        return new SourceContext(Optional.ofNullable(fetch), Clock.system(), "", false,
                PricingOptions.DEFAULT_TTL_MS, false);
    }

    public static SourceContext empty() {
        return new SourceContext(Optional.empty(), Clock.system(), "", false,
                PricingOptions.DEFAULT_TTL_MS, false);
    }
}
