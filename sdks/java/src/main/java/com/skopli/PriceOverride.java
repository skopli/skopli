package com.skopli;

import java.util.LinkedHashMap;
import java.util.Map;
import java.util.OptionalDouble;

/** A programmatic per-model price override - the highest-priority catalog. */
public record PriceOverride(
        String model,
        double input,
        double output,
        OptionalDouble cacheRead,
        OptionalDouble cacheWrite,
        OptionalDouble cacheWrite1h) {

    /** A flat override with no cache rates. */
    public static PriceOverride flat(String model, double input, double output) {
        return new PriceOverride(model, input, output, OptionalDouble.empty(),
                OptionalDouble.empty(), OptionalDouble.empty());
    }

    Map<String, Object> toWire() {
        Map<String, Object> m = new LinkedHashMap<>();
        m.put("model", model);
        m.put("input", input);
        m.put("output", output);
        cacheRead.ifPresent(v -> m.put("cacheRead", v));
        cacheWrite.ifPresent(v -> m.put("cacheWrite", v));
        cacheWrite1h.ifPresent(v -> m.put("cacheWrite1h", v));
        return m;
    }
}
