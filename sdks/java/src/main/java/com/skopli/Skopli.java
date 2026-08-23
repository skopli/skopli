package com.skopli;

import com.skopli.internal.Ffi;
import com.skopli.internal.Wire;
import com.skopli.json.Json;
import java.util.ArrayList;
import java.util.List;

/**
 * The skopli SDK entry point: static operations over the C ABI (ridden via
 * Java FFM). Sync and thread-safe; wrap in {@link java.util.concurrent.CompletableFuture}
 * for async. All data crosses as JSON marshalled to/from the idiomatic records
 * in this package; a non-zero native status surfaces as an unchecked
 * {@link SkopliException}.
 */
public final class Skopli {

    private Skopli() {}

    /** Force the native library to load eagerly (otherwise it loads on first
     * call). Useful to surface an {@link UnsatisfiedLinkError} at startup. */
    public static void load() {
        Ffi.init();
    }

    /** The ABI version integer (bumped on a breaking ABI change). */
    public static int abiVersion() {
        return Ffi.abiVersion();
    }

    /** The conformance schema version this build emits. */
    public static int schemaVersion() {
        return Ffi.schemaVersion();
    }

    /** The library semver string. */
    public static String version() {
        return Ffi.version();
    }

    /** The default on-disk cache directory (platform-specific). */
    public static String defaultCacheDir() {
        return Ffi.defaultCacheDir();
    }

    /** Detect which registered harnesses have readable data reachable from the
     * context. */
    public static Detection detectHarnesses(DetectOptions options) {
        String optsJson = Json.write(options.toWire());
        String out = Ffi.optsToJson(com.skopli.ffi.Skopli::ag_detect_harnesses, optsJson);
        return Detection.fromWire(Wire.asObject(Json.parse(out)));
    }

    /** Read usage across the selected harnesses, returning the
     * {events, diagnostics, skipped} envelope. */
    public static ReadUsageResult readUsage(ReadUsageOptions options) {
        String optsJson = Json.write(options.toWire());
        String out = Ffi.optsToJson(com.skopli.ffi.Skopli::ag_read_usage, optsJson);
        return ReadUsageResult.fromWire(Wire.asObject(Json.parse(out)));
    }

    /** Roll up an event batch by the option-named dimension. */
    public static List<Rollup> rollup(List<UsageEvent> events, RollupOptions options) {
        String eventsJson = eventsToJson(events);
        String optsJson = Json.write(options.toWire());
        String out = Ffi.rollup(eventsJson, optsJson);
        List<Rollup> result = new ArrayList<>();
        for (Object r : Wire.asArray(Json.parse(out))) {
            result.add(Rollup.fromWire(Wire.asObject(r)));
        }
        return List.copyOf(result);
    }

    /** Compute the USD cost of a token bundle under a flat/tiered price. */
    public static double costUsd(TokenCounts tokens, ModelPrice price) {
        String tokensJson = Json.write(tokens.toWire());
        String priceJson = Json.write(price.toWire());
        return Ffi.costUsd(tokensJson, priceJson);
    }

    /** Construct a {@link Pricing} handle. Any {@link PricingSource}s are
     * resolved facade-side and their catalogs merged before the FFI call. Use
     * try-with-resources ({@link Pricing} is {@link AutoCloseable}). */
    public static Pricing createPricing(PricingOptions options) {
        return Pricing.create(options);
    }

    static String eventsToJson(List<UsageEvent> events) {
        List<Object> arr = new ArrayList<>(events.size());
        for (UsageEvent e : events) {
            arr.add(e.toWire());
        }
        return Json.write(arr);
    }
}
