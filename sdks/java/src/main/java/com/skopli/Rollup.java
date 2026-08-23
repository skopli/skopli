package com.skopli;

import com.skopli.internal.Wire;
import java.util.Map;
import java.util.OptionalDouble;

/** One aggregated bucket produced by {@code rollup}. {@code costUsd} is present
 * only when at least one contributing event carried a recorded cost. */
public record Rollup(
        String key,
        TokenCounts tokens,
        long events,
        long turns,
        long calls,
        OptionalDouble costUsd) {

    static Rollup fromWire(Map<String, Object> m) {
        return new Rollup(
                Wire.asString(m.get("key")),
                TokenCounts.fromWire(Wire.asObject(m.get("tokens"))),
                Wire.asLong(m.get("events")),
                Wire.asLong(m.get("turns")),
                Wire.asLong(m.get("calls")),
                Wire.optDouble(m.get("costUsd")));
    }
}
