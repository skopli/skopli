package com.skopli;

import com.skopli.internal.Ffi;
import com.skopli.internal.Wire;
import com.skopli.json.Json;
import java.lang.foreign.MemorySegment;
import java.util.ArrayList;
import java.util.HashSet;
import java.util.LinkedHashMap;
import java.util.List;
import java.util.Map;
import java.util.Optional;
import java.util.Set;
import java.util.concurrent.locks.ReentrantLock;

/**
 * A live pricing handle wrapping a native catalog set. Thread-safe: a single
 * lock guards every native downcall, the TTL-expiry reload that swaps the native
 * handle, and {@link #close()}, so a downcall can never overlap a free.
 *
 * <p>Catalogs are memoized only within the TTL window; a query after
 * {@code now - loadedAt >= ttlMs} reloads the sources through their normal cache
 * lifecycle (each source's own disk cache keeps that cheap when still fresh),
 * builds a fresh native handle, swaps it in, and frees the old one. A long-lived
 * instance therefore picks up refreshed market prices, mirroring the TypeScript
 * reference. {@code refresh} stays one-shot: it applies only to the first load;
 * later TTL reloads use normal cache rules. A failed reload is not memoized.
 *
 * <p>Use with try-with-resources ({@code try (Pricing p =
 * Skopli.createPricing(opts)) { ... }}).
 */
public final class Pricing implements AutoCloseable {

    private final ReentrantLock lock = new ReentrantLock();

    // Retained source lifecycle state, so a query after TTL expiry can reload.
    private final List<Catalog> baseCatalogs;
    private final List<PricingSource> sources;
    private final PricingMode mode;
    private final List<PriceOverride> overrides;
    private final SourceContext ctx;
    private final long ttlMs;

    // one-shot refresh: applies to the first load only; sources whose refresh
    // fetch already completed are not refetched on a retry after a partial fail
    private boolean pendingRefresh;
    private final Set<PricingSource> refreshedSources = new HashSet<>();

    private MemorySegment handle;
    // epoch-millis (from the injected clock) when the current handle's catalog
    // load completed; null while no handle is loaded
    private Long loadedAt;
    private boolean closed;

    private Pricing(
            List<Catalog> baseCatalogs,
            List<PricingSource> sources,
            PricingMode mode,
            List<PriceOverride> overrides,
            SourceContext ctx,
            long ttlMs,
            boolean refresh) {
        this.baseCatalogs = baseCatalogs;
        this.sources = sources;
        this.mode = mode;
        this.overrides = overrides;
        this.ctx = ctx;
        this.ttlMs = ttlMs;
        this.pendingRefresh = refresh;
    }

    static Pricing create(PricingOptions options) {
        List<Catalog> baseCatalogs = List.copyOf(options.catalogs());
        List<PricingSource> sources =
                options.sources() == null ? Sources.builtin() : options.sources();
        Fetcher fetch = options.fetch().orElseGet(Sources::httpFetcher);
        Clock clock = options.clock().orElseGet(Clock::system);
        long ttlMs = options.ttlMs().orElse(PricingOptions.DEFAULT_TTL_MS);
        String cacheDir = options.cacheDir().isEmpty() ? Ffi.defaultCacheDir() : options.cacheDir();

        SourceContext ctx = new SourceContext(
                Optional.of(fetch), clock, cacheDir, options.offline(), ttlMs, options.refresh());

        Pricing pricing = new Pricing(
                baseCatalogs,
                sources,
                options.mode(),
                List.copyOf(options.overrides()),
                ctx,
                ttlMs,
                options.refresh());
        // Build the first handle eagerly so construction surfaces catalog errors.
        pricing.lock.lock();
        try {
            pricing.reload();
        } finally {
            pricing.lock.unlock();
        }
        return pricing;
    }

    /** The provenance of the loaded catalogs. */
    public List<CatalogInfo> catalogs() {
        lock.lock();
        try {
            ensureFresh();
            String out = Ffi.catalogInfo(handle);
            List<CatalogInfo> infos = new ArrayList<>();
            for (Object info : Wire.asArray(Json.parse(out))) {
                infos.add(CatalogInfo.fromWire(Wire.asObject(info)));
            }
            return List.copyOf(infos);
        } finally {
            lock.unlock();
        }
    }

    /** Price a set of pre-aggregated rollups, returning each rollup with its
     * {@link PriceLookup} attached (a hit with an optional
     * {@code tieredAggregate}, or a miss). The input rollups are the same shape
     * {@code rollup} emits. */
    public List<PricedRollup> priceRollups(List<Rollup> rollups) {
        lock.lock();
        try {
            ensureFresh();
            List<Object> arr = new ArrayList<>(rollups.size());
            for (Rollup r : rollups) {
                arr.add(rollupToWire(r));
            }
            String out = Ffi.priceRollups(handle, Json.write(arr));
            List<PricedRollup> result = new ArrayList<>();
            for (Object r : Wire.asArray(Json.parse(out))) {
                result.add(PricedRollup.fromWire(Wire.asObject(r)));
            }
            return List.copyOf(result);
        } finally {
            lock.unlock();
        }
    }

    /** Resolve a single model's price, returning a {@link PriceLookup.PriceHit}
     * or a {@link PriceLookup.PriceMiss}. */
    public PriceLookup lookupModel(String model) {
        lock.lock();
        try {
            ensureFresh();
            String out = Ffi.lookupModel(handle, model);
            return PriceLookup.fromWire(Wire.asObject(Json.parse(out)));
        } finally {
            lock.unlock();
        }
    }

    /** Price an event batch grouped by the option dimension, returning each
     * bucket with its {@link PriceLookup} attached. */
    public List<PricedRollup> priceEvents(List<UsageEvent> events, RollupOptions options) {
        lock.lock();
        try {
            ensureFresh();
            String eventsJson = Skopli.eventsToJson(events);
            String optsJson = Json.write(options.toWire());
            String out = Ffi.priceEvents(handle, eventsJson, optsJson);
            List<PricedRollup> result = new ArrayList<>();
            for (Object r : Wire.asArray(Json.parse(out))) {
                result.add(PricedRollup.fromWire(Wire.asObject(r)));
            }
            return List.copyOf(result);
        } finally {
            lock.unlock();
        }
    }

    /** Free the native handle. Idempotent. */
    @Override
    public void close() {
        lock.lock();
        try {
            if (!closed) {
                if (handle != null) {
                    Ffi.pricingFree(handle);
                    handle = null;
                }
                closed = true;
            }
        } finally {
            lock.unlock();
        }
    }

    // -- lifecycle (all under lock) --------------------------------------

    private void ensureOpen() {
        if (closed) {
            throw new IllegalStateException("Pricing handle is closed");
        }
    }

    /** Reload catalogs through the normal source lifecycle if the current handle
     * is expired (or absent). Called with the lock held. */
    private void ensureFresh() {
        ensureOpen();
        boolean expired =
                loadedAt != null && ctx.clock().nowMillis() - loadedAt >= ttlMs;
        if (handle == null || expired) {
            reload();
        }
    }

    /** Reload sources, build a new native handle, swap it in, and free the old
     * one. Called with the lock held. A failed reload leaves no handle memoized:
     * the next query retries (and a failed refresh retry still honours refresh). */
    private void reload() {
        boolean refreshing = pendingRefresh;
        pendingRefresh = false;
        List<Catalog> catalogs = new ArrayList<>(baseCatalogs);
        try {
            for (PricingSource source : sources) {
                // per-source refresh: a retry after a partial failure skips
                // sources whose refresh fetch already completed
                boolean refresh = refreshing && !refreshedSources.contains(source);
                SourceContext sourceCtx = refresh == ctx.refresh()
                        ? ctx
                        : new SourceContext(
                                ctx.fetch(), ctx.clock(), ctx.cacheDir(),
                                ctx.offline(), ctx.ttlMs(), refresh);
                Optional<Catalog> produced;
                try {
                    produced = source.load(sourceCtx);
                } catch (Exception e) {
                    throw new CatalogException(
                            "source \"" + source.name() + "\" failed: " + e.getMessage());
                }
                if (refresh) {
                    refreshedSources.add(source);
                }
                produced.ifPresent(catalogs::add);
            }

            Map<String, Object> wire = new LinkedHashMap<>();
            wire.put("mode", mode.id());
            if (!overrides.isEmpty()) {
                List<Object> arr = new ArrayList<>();
                for (PriceOverride o : overrides) {
                    arr.add(o.toWire());
                }
                wire.put("overrides", arr);
            }
            if (!catalogs.isEmpty()) {
                List<Object> arr = new ArrayList<>();
                for (Catalog c : catalogs) {
                    arr.add(c.toWire());
                }
                wire.put("catalogs", arr);
            }
            wire.put("builtinSources", false);

            MemorySegment next = Ffi.pricingNew(Json.write(wire));
            MemorySegment previous = handle;
            handle = next;
            loadedAt = ctx.clock().nowMillis();
            if (previous != null) {
                Ffi.pricingFree(previous);
            }
        } catch (RuntimeException e) {
            // A rejected load is not memoized, so the next query retries: this
            // failing query propagates the error (mirroring TS, where a rejected
            // catalogs load rejects that query's promise and the next call
            // retries). If a prior handle exists its completion stamp is left
            // untouched: it is already expired (that is why we reloaded), so the
            // next query sees expired and retries rather than serving it as a
            // memoized fresh handle. Only when there is no usable handle is
            // loadedAt cleared. A rejected refresh restores the pending refresh
            // so the retry honours it.
            if (handle == null) {
                loadedAt = null;
            }
            if (refreshing) {
                pendingRefresh = true;
            }
            throw e;
        }
    }

    /** A {@link Rollup} as a wire map matching the rollup grammar the core
     * consumes. */
    private static Map<String, Object> rollupToWire(Rollup r) {
        Map<String, Object> m = new LinkedHashMap<>();
        m.put("key", r.key());
        m.put("tokens", r.tokens().toWire());
        m.put("events", r.events());
        m.put("turns", r.turns());
        m.put("calls", r.calls());
        r.costUsd().ifPresent(v -> m.put("costUsd", v));
        return m;
    }
}
