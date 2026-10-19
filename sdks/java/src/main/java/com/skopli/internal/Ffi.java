package com.skopli.internal;

import com.skopli.CatalogException;
import com.skopli.InternalException;
import com.skopli.InvalidArgumentException;
import com.skopli.ffi.AgBuf;
import com.skopli.ffi.Skopli;
import com.skopli.json.Json;
import java.lang.foreign.Arena;
import java.lang.foreign.MemorySegment;
import java.lang.foreign.ValueLayout;
import java.nio.charset.StandardCharsets;
import java.util.LinkedHashMap;
import java.util.List;
import java.util.Map;

/**
 * The single place that touches the jextract FFM layer. Owns all
 * {@code MemorySegment}/{@code AgBuf}/C-string ownership and the
 * {@code AgStatus} -> exception mapping, so the public facade stays pure Java.
 *
 * <p>Inputs are borrowed for the call only (allocated in the call {@link Arena}).
 * {@code AgBuf} and {@code char*} outputs are library-owned: copied to a Java
 * {@code String}, then freed with {@code ag_buf_free} / {@code ag_string_free}.
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

    public static int abiVersion() {
        return Skopli.ag_abi_version();
    }

    public static int schemaVersion() {
        return Skopli.ag_schema_version();
    }

    public static String version() {
        return cString(Skopli.ag_version());
    }

    public static String defaultCacheDir() {
        try (Arena arena = Arena.ofConfined()) {
            MemorySegment out = arena.allocate(ValueLayout.ADDRESS);
            check(Skopli.ag_default_cache_dir(out));
            return takeString(out.get(ValueLayout.ADDRESS, 0));
        }
    }

    /** {@code ag_detect_harnesses}/{@code ag_read_usage} shape: (opts) -> JSON. */
    public static String optsToJson(OptsFn fn, String optsJson) {
        try (Arena arena = Arena.ofConfined()) {
            Buf opts = Buf.of(arena, optsJson);
            MemorySegment out = AgBuf.allocate(arena);
            check(fn.call(opts.seg, opts.len, out));
            return takeBuf(out);
        }
    }

    public static String rollup(String eventsJson, String optsJson) {
        try (Arena arena = Arena.ofConfined()) {
            Buf events = Buf.of(arena, eventsJson);
            Buf opts = Buf.of(arena, optsJson);
            MemorySegment out = AgBuf.allocate(arena);
            check(Skopli.ag_rollup(events.seg, events.len, opts.seg, opts.len, out));
            return takeBuf(out);
        }
    }

    public static double costUsd(String tokensJson, String priceJson) {
        try (Arena arena = Arena.ofConfined()) {
            Buf tokens = Buf.of(arena, tokensJson);
            Buf price = Buf.of(arena, priceJson);
            MemorySegment out = arena.allocate(ValueLayout.JAVA_DOUBLE);
            check(Skopli.ag_cost_usd(tokens.seg, tokens.len, price.seg, price.len, out));
            return out.get(ValueLayout.JAVA_DOUBLE, 0);
        }
    }

    /** {@code ag_pricing_new}: returns the opaque handle segment. */
    public static MemorySegment pricingNew(String optsJson) {
        try (Arena arena = Arena.ofConfined()) {
            Buf opts = Buf.of(arena, optsJson);
            MemorySegment out = arena.allocate(ValueLayout.ADDRESS);
            check(Skopli.ag_pricing_new(opts.seg, opts.len, out));
            return out.get(ValueLayout.ADDRESS, 0);
        }
    }

    public static void pricingFree(MemorySegment handle) {
        Skopli.ag_pricing_free(handle);
    }

    /** Count the prices the core parser extracts from one raw source payload by
     * building a throwaway native handle holding exactly that catalog. Returns 0
     * when the build fails or the payload parses to no prices. The count comes
     * from the core parser, so the facade never reimplements one. */
    public static long probeModelCount(String source, String format, Object payload) {
        Map<String, Object> catalog = new LinkedHashMap<>();
        catalog.put("source", source);
        catalog.put("format", format);
        catalog.put("payload", payload);
        Map<String, Object> opts = new LinkedHashMap<>();
        opts.put("mode", "calculate");
        opts.put("catalogs", List.of(catalog));
        opts.put("builtinSources", false);
        MemorySegment handle;
        try {
            handle = pricingNew(Json.write(opts));
        } catch (RuntimeException e) {
            return 0L;
        }
        try {
            for (Object info : Wire.asArray(Json.parse(catalogInfo(handle)))) {
                Map<String, Object> m = Wire.asObject(info);
                if (source.equals(m.get("source"))) {
                    return Wire.asLong(m.get("models"));
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
            Buf events = Buf.of(arena, eventsJson);
            Buf opts = Buf.of(arena, optsJson);
            MemorySegment out = AgBuf.allocate(arena);
            check(Skopli.ag_pricing_price_events(handle, events.seg, events.len, opts.seg, opts.len, out));
            return takeBuf(out);
        }
    }

    public static String priceRollups(MemorySegment handle, String rollupsJson) {
        try (Arena arena = Arena.ofConfined()) {
            Buf rollups = Buf.of(arena, rollupsJson);
            MemorySegment out = AgBuf.allocate(arena);
            check(Skopli.ag_pricing_price_rollups(handle, rollups.seg, rollups.len, out));
            return takeBuf(out);
        }
    }

    public static String lookupModel(MemorySegment handle, String model) {
        try (Arena arena = Arena.ofConfined()) {
            Buf name = Buf.of(arena, model);
            MemorySegment out = AgBuf.allocate(arena);
            check(Skopli.ag_pricing_lookup_model(handle, name.seg, name.len, out));
            return takeBuf(out);
        }
    }

    public static String catalogInfo(MemorySegment handle) {
        try (Arena arena = Arena.ofConfined()) {
            MemorySegment out = AgBuf.allocate(arena);
            check(Skopli.ag_pricing_catalog_info(handle, out));
            return takeBuf(out);
        }
    }

    /** A jextract {@code ag_detect_harnesses}/{@code ag_read_usage} handle. */
    @FunctionalInterface
    public interface OptsFn {
        int call(MemorySegment optsJson, long optsLen, MemorySegment out);
    }

    /** A UTF-8 view of a Java string for the (ptr, len) ABI, borrowed from an
     * arena. A null string is {@code (NULL, 0)}, which the ABI reads as "all
     * defaults". */
    private record Buf(MemorySegment seg, long len) {
        static Buf of(Arena arena, String s) {
            if (s == null) {
                return new Buf(MemorySegment.NULL, 0);
            }
            byte[] bytes = s.getBytes(StandardCharsets.UTF_8);
            MemorySegment seg = arena.allocate(Math.max(bytes.length, 1));
            MemorySegment.copy(bytes, 0, seg, ValueLayout.JAVA_BYTE, 0, bytes.length);
            return new Buf(seg, bytes.length);
        }
    }

    private static String takeBuf(MemorySegment bufStruct) {
        MemorySegment ptr = AgBuf.ptr(bufStruct);
        long len = AgBuf.len(bufStruct);
        String result = ptr.address() == 0 || len == 0
                ? ""
                : new String(ptr.reinterpret(len).toArray(ValueLayout.JAVA_BYTE), StandardCharsets.UTF_8);
        Skopli.ag_buf_free(bufStruct);
        return result;
    }

    private static String takeString(MemorySegment str) {
        String value = cString(str);
        Skopli.ag_string_free(str);
        return value;
    }

    private static String cString(MemorySegment p) {
        if (p == null || p.address() == 0) {
            return "";
        }
        return p.reinterpret(Long.MAX_VALUE).getString(0);
    }

    private static String lastError() {
        try (Arena arena = Arena.ofConfined()) {
            MemorySegment out = arena.allocate(ValueLayout.ADDRESS);
            if (Skopli.ag_last_error_message(out) != 0) {
                return "";
            }
            return takeString(out.get(ValueLayout.ADDRESS, 0));
        }
    }

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
}
