package com.skopli;

import com.skopli.internal.Wire;
import java.util.ArrayList;
import java.util.List;
import java.util.Map;
import java.util.Optional;
import java.util.OptionalDouble;

/**
 * The outcome of pricing a group of events: a {@link PriceHit} (a matching
 * catalog price was found and applied) or a {@link PriceMiss} (no price
 * matched). The {@code priced} boolean on the wire discriminates the two;
 * switch over the sealed subtypes with pattern matching.
 */
public sealed interface PriceLookup permits PriceLookup.PriceHit, PriceLookup.PriceMiss {

    /** Whether a price was found. */
    boolean priced();

    /** The dollar cost attributed to the group. */
    OptionalDouble usd();

    /** A matched, priced group. */
    record PriceHit(
            String model,
            Optional<String> key,
            Optional<ModelPrice> price,
            Optional<String> source,
            Optional<String> fetchedAt,
            OptionalDouble usd,
            boolean tieredAggregate) implements PriceLookup {

        @Override
        public boolean priced() {
            return true;
        }
    }

    /** An unmatched group; {@code attempted} lists the lookup keys tried. */
    record PriceMiss(
            String model,
            List<String> attempted,
            Optional<String> reason,
            Optional<String> key,
            OptionalDouble usd) implements PriceLookup {

        @Override
        public boolean priced() {
            return false;
        }
    }

    /** Parse a {@code pricing} sub-object from a priced-rollup wire entry. */
    static PriceLookup fromWire(Map<String, Object> m) {
        boolean priced = Wire.asBool(m.get("priced"));
        if (priced) {
            Optional<ModelPrice> price = m.get("price") == null
                    ? Optional.empty()
                    : Optional.of(ModelPrice.fromWire(Wire.asObject(m.get("price"))));
            return new PriceHit(
                    Wire.asString(m.get("model")),
                    Wire.optString(m.get("key")),
                    price,
                    Wire.optString(m.get("source")),
                    Wire.optString(m.get("fetchedAt")),
                    Wire.optDouble(m.get("usd")),
                    Wire.asBool(m.get("tieredAggregate")));
        }
        List<String> attempted = new ArrayList<>();
        if (m.get("attempted") != null) {
            for (Object a : Wire.asArray(m.get("attempted"))) {
                attempted.add(Wire.asString(a));
            }
        }
        return new PriceMiss(
                Wire.asString(m.get("model")),
                List.copyOf(attempted),
                Wire.optString(m.get("reason")),
                Wire.optString(m.get("key")),
                Wire.optDouble(m.get("usd")));
    }
}
