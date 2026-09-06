package com.skopli;

import com.skopli.internal.Wire;
import java.util.Map;
import java.util.Optional;

/** Provenance of one catalog held by a {@link Pricing} handle: its source name,
 * fetch timestamp (absent for the override catalog), and model count. */
public record CatalogInfo(String source, Optional<String> fetchedAt, long models) {

    static CatalogInfo fromWire(Map<String, Object> m) {
        return new CatalogInfo(
                Wire.asString(m.get("source")),
                Wire.optString(m.get("fetchedAt")),
                Wire.asLong(m.get("models")));
    }
}
