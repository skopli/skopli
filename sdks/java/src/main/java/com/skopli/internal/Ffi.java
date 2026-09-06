package com.skopli.internal;

import com.skopli.SkopliException;
import com.skopli.CatalogException;
import com.skopli.InternalException;
import com.skopli.InvalidArgumentException;
import com.skopli.ffi.AgBuf;
import com.skopli.ffi.Skopli;
import java.lang.foreign.Arena;
import java.lang.foreign.MemorySegment;
import java.lang.foreign.ValueLayout;
import java.nio.charset.StandardCharsets;

/**
 * The single place that touches the jextract FFM layer. Owns all
 * {@code MemorySegment}/{@code AgBuf}/C-string ownership and the
 * {@code AgStatus} -> exception mapping, so the public facade stays pure Java.
 *
 * <p>Buffer discipline (mirroring the C ABI contract):
 * <ul>
 *   <li>Inputs (JSON, options) are borrowed for the call only - allocated in the
 *       call {@link Arena} and freed when it closes.</li>
 *   <li>{@code AgBuf} outputs are library-owned; copied to a Java {@code String}
 *       then freed with {@code ag_buf_free}.</li>
 *   <li>{@code char*} outputs are library-owned; copied then freed with
 *       {@code ag_string_free}.</li>
 * </ul>
 * All methods are static and thread-safe (each call owns its own arena; the
 * native side holds no global mutable state beyond the per-thread error).
 */
public final class Ffi {

    private Ffi() {}

    static {
        NativeLibrary.ensureLoaded();
    }

    /** Force class init (and thus the native load) without invoking a call. */
    public static void init() {}

    // -- meta -------------------------------------------------------------

    public static int abiVersion() {
        return Skopli.ag_abi_version();
    }

    public static int schemaVersion() {
        return Skopli.ag_schema_version();
    }

    public static String version() {
        MemorySegment p = Skopli.ag_version();
        return cString(p);
    }

    public static String defaultCacheDir() {
        try (Arena arena = Arena.ofConfined()) {
            MemorySegment out = arena.allocate(ValueLayout.ADDRESS);
            int status = Skopli.ag_default_cache_dir(out);
            check(status);
            MemorySegment str = out.get(ValueLayout.ADDRESS, 0);
            String value = cString(str);
            Skopli.ag_string_free(str);
            return value;
        }
    }

    // -- JSON-out pipeline calls ------------------------------------------

    /** {@code ag_detect_harnesses}/{@code ag_read_usage} shape: (opts) -> JSON. */
    public static String optsToJson(OptsFn fn, String optsJson) {
        try (Arena arena = Arena.ofConfined()) {
            MemorySegment opts = toBuffer(arena, optsJson);
            MemorySegment out = AgBuf.allocate(arena);
            int status = fn.call(opts, opts == MemorySegment.NULL ? 0 : optsJson.getBytes(StandardCharsets.UTF_8).length, out);
            check(status);
            return takeBuf(out);
        }
    }

    /** {@code ag_rollup} shape: (events, opts) -> JSON. */
    public static String rollup(String eventsJson, String optsJson) {
        try (Arena arena = Arena.ofConfined()) {
            byte[] eb = eventsJson.getBytes(StandardCharsets.UTF_8);
            byte[] ob = optsJson.getBytes(StandardCharsets.UTF_8);
            MemorySegment events = toBuffer(arena, eventsJson);
            MemorySegment opts = toBuffer(arena, optsJson);
            MemorySegment out = AgBuf.allocate(arena);
            int status = Skopli.ag_rollup(events, eb.length, opts, ob.length, out);
            check(status);
            return takeBuf(out);
        }
    }

    /** {@code ag_cost_usd} shape: (tokens, price) -> double. */
    public static double costUsd(String tokensJson, String priceJson) {
        try (Arena arena = Arena.ofConfined()) {
            byte[] tb = tokensJson.getBytes(StandardCharsets.UTF_8);
            byte[] pb = priceJson.getBytes(StandardCharsets.UTF_8);
            MemorySegment tokens = toBuffer(arena, tokensJson);
            MemorySegment price = toBuffer(arena, priceJson);
            MemorySegment out = arena.allocate(ValueLayout.JAVA_DOUBLE);
            int status = Skopli.ag_cost_usd(tokens, tb.length, price, pb.length, out);
            check(status);
            return out.get(ValueLayout.JAVA_DOUBLE, 0);
        }
    }

    // -- pricing handle ---------------------------------------------------

    /** {@code ag_pricing_new}: returns the opaque handle segment. */
    public static MemorySegment pricingNew(String optsJson) {
        try (Arena arena = Arena.ofConfined()) {
            byte[] ob = optsJson.getBytes(StandardCharsets.UTF_8);
            MemorySegment opts = toBuffer(arena, optsJson);
            MemorySegment out = arena.allocate(ValueLayout.ADDRESS);
            int status = Skopli.ag_pricing_new(opts, ob.length, out);
            check(status);
            return out.get(ValueLayout.ADDRESS, 0);
        }
    }

    public static void pricingFree(MemorySegment handle) {
        Skopli.ag_pricing_free(handle);
    }

    /** Probe a single raw source payload through the core parser: build a
     * throwaway native handle with exactly one raw catalog
     * ({@code {source, fetchedAt, format, payload}}, {@code builtinSources:false},
     * mode {@code calculate}), read that catalog's {@code models} count, and free
     * the handle. Returns the model count, or 0 when the build fails or the
     * payload parses to no prices. Never reimplements a source parser. */
    public static long probeModelCount(String source, String format, Object payload) {
        java.util.Map<String, Object> catalog = new java.util.LinkedHashMap<>();
        catalog.put("source", source);
        catalog.put("format", format);
        catalog.put("payload", payload);
        java.util.Map<String, Object> opts = new java.util.LinkedHashMap<>();
        opts.put("mode", "calculate");
        opts.put("catalogs", java.util.List.of(catalog));
        opts.put("builtinSources", false);
        MemorySegment handle;
        try {
            handle = pricingNew(com.skopli.json.Json.write(opts));
        } catch (RuntimeException e) {
            return 0L;
        }
        try {
            String out = catalogInfo(handle);
            for (Object info : com.skopli.json.Json.parse(out) instanceof java.util.List<?> l
                    ? l
                    : java.util.List.of()) {
                if (info instanceof java.util.Map<?, ?> m
                        && source.equals(m.get("source"))) {
                    Object models = m.get("models");
                    if (models instanceof Number n) {
                        return n.longValue();
                    }
                }
            }
            return 0L;
        } catch (RuntimeException e) {
            return 0L;
        } finally {
            pricingFree(handle);
        }
    }

    public static String priceEvents(MemorySegment handle, String eventsJson, String optsJson) {
        try (Arena arena = Arena.ofConfined()) {
            byte[] eb = eventsJson.getBytes(StandardCharsets.UTF_8);
            byte[] ob = optsJson.getBytes(StandardCharsets.UTF_8);
            MemorySegment events = toBuffer(arena, eventsJson);
            MemorySegment opts = toBuffer(arena, optsJson);
            MemorySegment out = AgBuf.allocate(arena);
            int status = Skopli.ag_pricing_price_events(handle, events, eb.length, opts, ob.length, out);
            check(status);
            return takeBuf(out);
        }
    }

    public static String priceRollups(MemorySegment handle, String rollupsJson) {
        try (Arena arena = Arena.ofConfined()) {
            byte[] rb = rollupsJson.getBytes(StandardCharsets.UTF_8);
            MemorySegment rollups = toBuffer(arena, rollupsJson);
            MemorySegment out = AgBuf.allocate(arena);
            int status = Skopli.ag_pricing_price_rollups(handle, rollups, rb.length, out);
            check(status);
            return takeBuf(out);
        }
    }

    public static String lookupModel(MemorySegment handle, String model) {
        try (Arena arena = Arena.ofConfined()) {
            byte[] mb = model.getBytes(StandardCharsets.UTF_8);
            MemorySegment name = toBuffer(arena, model);
            MemorySegment out = AgBuf.allocate(arena);
            int status = Skopli.ag_pricing_lookup_model(handle, name, mb.length, out);
            check(status);
            return takeBuf(out);
        }
    }

    public static String catalogInfo(MemorySegment handle) {
        try (Arena arena = Arena.ofConfined()) {
            MemorySegment out = AgBuf.allocate(arena);
            int status = Skopli.ag_pricing_catalog_info(handle, out);
            check(status);
            return takeBuf(out);
        }
    }

    /** A jextract {@code ag_detect_harnesses}/{@code ag_read_usage} handle. */
    @FunctionalInterface
    public interface OptsFn {
        int call(MemorySegment optsJson, long optsLen, MemorySegment out);
    }

    // -- helpers ----------------------------------------------------------

    /** Allocate a NUL-free UTF-8 view of {@code s} for the (ptr,len) ABI, or
     * NULL for a null string (= "all defaults"). The ABI reads only len bytes;
     * we allocate exactly the bytes (no trailing NUL needed but harmless). */
    private static MemorySegment toBuffer(Arena arena, String s) {
        if (s == null) {
            return MemorySegment.NULL;
        }
        byte[] bytes = s.getBytes(StandardCharsets.UTF_8);
        MemorySegment seg = arena.allocate(bytes.length == 0 ? 1 : bytes.length);
        MemorySegment.copy(bytes, 0, seg, ValueLayout.JAVA_BYTE, 0, bytes.length);
        return seg;
    }

    /** Take ownership of an out-{@code AgBuf}: copy its bytes to a String, then
     * free it with {@code ag_buf_free}. An empty buffer (ptr==NULL) yields "". */
    private static String takeBuf(MemorySegment bufStruct) {
        MemorySegment ptr = AgBuf.ptr(bufStruct);
        long len = AgBuf.len(bufStruct);
        String result;
        if (ptr.address() == 0 || len == 0) {
            result = "";
        } else {
            MemorySegment bytes = ptr.reinterpret(len);
            byte[] copy = bytes.toArray(ValueLayout.JAVA_BYTE);
            result = new String(copy, StandardCharsets.UTF_8);
        }
        Skopli.ag_buf_free(bufStruct);
        return result;
    }

    /** Read a NUL-terminated library C string into a Java String (no free). */
    private static String cString(MemorySegment p) {
        if (p == null || p.address() == 0) {
            return "";
        }
        return p.reinterpret(Long.MAX_VALUE).getString(0);
    }

    /** Read this thread's {@code ag_last_error_message}, freeing it. */
    private static String lastError() {
        try (Arena arena = Arena.ofConfined()) {
            MemorySegment out = arena.allocate(ValueLayout.ADDRESS);
            int status = Skopli.ag_last_error_message(out);
            if (status != 0) {
                return "";
            }
            MemorySegment str = out.get(ValueLayout.ADDRESS, 0);
            if (str.address() == 0) {
                return "";
            }
            String msg = cString(str);
            Skopli.ag_string_free(str);
            return msg;
        }
    }

    /** Map a non-zero {@code AgStatus} to the corresponding exception, pulling
     * the thread-local detail message. */
    private static void check(int status) {
        if (status == 0) {
            return;
        }
        String detail = lastError();
        String msg = detail.isEmpty() ? ("skopli status " + status) : detail;
        throw switch (status) {
            case 1 -> new InvalidArgumentException(msg);
            case 2 -> new CatalogException(msg);
            default -> new InternalException(msg);
        };
    }

    /** Ensures {@link SkopliException} is referenced (documentation of the
     * sealed base the mapping produces). */
    static Class<?> baseException() {
        return SkopliException.class;
    }
}
