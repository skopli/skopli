package com.skopli;

import com.skopli.internal.Wire;
import java.util.Map;

/** A {@link Rollup} bucket augmented with its {@link PriceLookup} outcome, as
 * produced by {@link Pricing#priceEvents}. */
public record PricedRollup(Rollup rollup, PriceLookup pricing) {

    static PricedRollup fromWire(Map<String, Object> m) {
        return new PricedRollup(
                Rollup.fromWire(m),
                PriceLookup.fromWire(Wire.asObject(m.get("pricing"))));
    }
}
