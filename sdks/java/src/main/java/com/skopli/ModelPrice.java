package com.skopli;

import com.skopli.internal.Wire;
import java.util.ArrayList;
import java.util.LinkedHashMap;
import java.util.List;
import java.util.Map;
import java.util.Optional;
import java.util.OptionalDouble;

/**
 * A model's price (per-million-token rates). {@code input}/{@code output} are
 * required; cache rates and {@code tiers} are optional. A flat price has no
 * tiers.
 */
public record ModelPrice(
        double input,
        double output,
        OptionalDouble cacheRead,
        OptionalDouble cacheWrite,
        OptionalDouble cacheWrite1h,
        Optional<List<PriceTier>> tiers,
        Optional<String> tierMode) {

    /** A flat price with no cache rates or tiers. */
    public static ModelPrice flat(double input, double output) {
        return new ModelPrice(input, output, OptionalDouble.empty(), OptionalDouble.empty(),
                OptionalDouble.empty(), Optional.empty(), Optional.empty());
    }

    static ModelPrice fromWire(Map<String, Object> m) {
        Optional<List<PriceTier>> tiers = Optional.empty();
        Object t = m.get("tiers");
        if (t != null) {
            List<PriceTier> parsed = new ArrayList<>();
            for (Object tier : Wire.asArray(t)) {
                parsed.add(PriceTier.fromWire(Wire.asObject(tier)));
            }
            tiers = Optional.of(List.copyOf(parsed));
        }
        return new ModelPrice(
                Wire.asDouble(m.get("input")),
                Wire.asDouble(m.get("output")),
                Wire.optDouble(m.get("cacheRead")),
                Wire.optDouble(m.get("cacheWrite")),
                Wire.optDouble(m.get("cacheWrite1h")),
                tiers,
                Wire.optString(m.get("tierMode")));
    }

    Map<String, Object> toWire() {
        Map<String, Object> m = new LinkedHashMap<>();
        m.put("input", input);
        m.put("output", output);
        cacheRead.ifPresent(v -> m.put("cacheRead", v));
        cacheWrite.ifPresent(v -> m.put("cacheWrite", v));
        cacheWrite1h.ifPresent(v -> m.put("cacheWrite1h", v));
        tiers.ifPresent(ts -> {
            List<Object> arr = new ArrayList<>();
            for (PriceTier tier : ts) {
                arr.add(tier.toWire());
            }
            m.put("tiers", arr);
        });
        tierMode.ifPresent(v -> m.put("tierMode", v));
        return m;
    }
}
