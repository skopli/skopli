package com.skopli;

import com.skopli.internal.Wire;
import java.util.LinkedHashMap;
import java.util.Map;
import java.util.OptionalLong;

/**
 * Token counters for a single usage event. Counters are non-negative integers.
 * {@code cacheWrite1h} is absent (empty) when the source omits the split.
 */
public record TokenCounts(
        long input,
        long output,
        long cacheRead,
        long cacheWrite,
        OptionalLong cacheWrite1h,
        long reasoning) {

    /** A flat count with no 1h cache-write split. */
    public static TokenCounts of(long input, long output, long cacheRead, long cacheWrite, long reasoning) {
        return new TokenCounts(input, output, cacheRead, cacheWrite, OptionalLong.empty(), reasoning);
    }

    static TokenCounts fromWire(Map<String, Object> m) {
        return new TokenCounts(
                Wire.asLong(m.get("input")),
                Wire.asLong(m.get("output")),
                Wire.asLong(m.get("cacheRead")),
                Wire.asLong(m.get("cacheWrite")),
                Wire.optLong(m.get("cacheWrite1h")),
                Wire.asLong(m.get("reasoning")));
    }

    Map<String, Object> toWire() {
        Map<String, Object> m = new LinkedHashMap<>();
        m.put("input", input);
        m.put("output", output);
        m.put("cacheRead", cacheRead);
        m.put("cacheWrite", cacheWrite);
        cacheWrite1h.ifPresent(v -> m.put("cacheWrite1h", v));
        m.put("reasoning", reasoning);
        return m;
    }
}
