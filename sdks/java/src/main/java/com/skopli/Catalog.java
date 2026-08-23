package com.skopli;

import java.util.LinkedHashMap;
import java.util.Map;
import java.util.Optional;

/**
 * A pre-fetched pricing catalog handed to the core. Two forms (mirroring the C
 * ABI {@code ag_pricing_new} contract):
 * <ul>
 *   <li><b>parsed</b>: {@code prices} is an already-parsed model->price map;</li>
 *   <li><b>raw</b>: {@code format} + {@code payload} carry a raw source payload
 *       the core parses ({@code "openrouter"}, {@code "litellm"},
 *       {@code "modelsdev"}).</li>
 * </ul>
 * A facade-side {@link PricingSource} produces one of these.
 */
public record Catalog(
        String source,
        Optional<String> fetchedAt,
        Optional<Map<String, ModelPrice>> prices,
        Optional<String> format,
        Optional<Object> payload) {

    /** A catalog from an already-parsed price map. */
    public static Catalog parsed(String source, String fetchedAt, Map<String, ModelPrice> prices) {
        return new Catalog(source, Optional.ofNullable(fetchedAt), Optional.of(Map.copyOf(prices)),
                Optional.empty(), Optional.empty());
    }

    /** A catalog from a raw source payload the core will parse. {@code payload}
     * is a parsed JSON value tree (Map/List/...), as produced by
     * {@link com.skopli.json.Json#parse}. */
    public static Catalog raw(String source, String fetchedAt, String format, Object payload) {
        return new Catalog(source, Optional.ofNullable(fetchedAt), Optional.empty(),
                Optional.of(format), Optional.of(payload));
    }

    Map<String, Object> toWire() {
        Map<String, Object> m = new LinkedHashMap<>();
        m.put("source", source);
        fetchedAt.ifPresent(v -> m.put("fetchedAt", v));
        prices.ifPresent(p -> {
            Map<String, Object> priceMap = new LinkedHashMap<>();
            for (Map.Entry<String, ModelPrice> e : p.entrySet()) {
                priceMap.put(e.getKey(), e.getValue().toWire());
            }
            m.put("prices", priceMap);
        });
        format.ifPresent(v -> m.put("format", v));
        payload.ifPresent(v -> m.put("payload", v));
        return m;
    }
}
