package com.skopli;

import com.skopli.internal.Wire;
import java.util.LinkedHashMap;
import java.util.Map;
import java.util.OptionalDouble;

/** One pricing tier (per-million-token rates above a cumulative token
 * threshold). Cache rates are optional. */
public record PriceTier(
        double threshold,
        double input,
        double output,
        OptionalDouble cacheRead,
        OptionalDouble cacheWrite,
        OptionalDouble cacheWrite1h) {

    static PriceTier fromWire(Map<String, Object> m) {
        return new PriceTier(
                Wire.asDouble(m.get("threshold")),
                Wire.asDouble(m.get("input")),
                Wire.asDouble(m.get("output")),
                Wire.optDouble(m.get("cacheRead")),
                Wire.optDouble(m.get("cacheWrite")),
                Wire.optDouble(m.get("cacheWrite1h")));
    }

    Map<String, Object> toWire() {
        Map<String, Object> m = new LinkedHashMap<>();
        m.put("threshold", threshold);
        m.put("input", input);
        m.put("output", output);
        cacheRead.ifPresent(v -> m.put("cacheRead", v));
        cacheWrite.ifPresent(v -> m.put("cacheWrite", v));
        cacheWrite1h.ifPresent(v -> m.put("cacheWrite1h", v));
        return m;
    }
}
