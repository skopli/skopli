package com.skopli;

import java.util.LinkedHashMap;
import java.util.Map;
import java.util.Objects;
import java.util.Optional;

/** Options for {@link Skopli#rollup} / {@link Pricing#priceEvents}: the
 * grouping dimension, an optional IANA timezone for day bucketing and
 * block-start hour flooring (the system timezone when absent), and an
 * optional billing-block width for {@link RollupBy#BLOCK}. */
public record RollupOptions(RollupBy by, Optional<String> tz, Optional<Long> blockMs) {

    /** The default billing-block width (five hours) for {@link RollupBy#BLOCK}. */
    public static final long DEFAULT_BLOCK_MS = 18_000_000L;

    public RollupOptions {
        Objects.requireNonNull(by, "by");
    }

    /** Roll up by the given dimension with system-timezone day bucketing. */
    public static RollupOptions by(RollupBy by) {
        return new RollupOptions(by, Optional.empty(), Optional.empty());
    }

    /** Roll up by the given dimension with an explicit timezone. */
    public static RollupOptions by(RollupBy by, String tz) {
        return new RollupOptions(by, Optional.ofNullable(tz), Optional.empty());
    }

    /** Roll up into billing blocks of the given width (ms), anchored in the
     * system timezone. */
    public static RollupOptions block(long blockMs) {
        return new RollupOptions(RollupBy.BLOCK, Optional.empty(), Optional.of(blockMs));
    }

    /** Roll up into billing blocks of the given width (ms), anchored in {@code tz}. */
    public static RollupOptions block(long blockMs, String tz) {
        return new RollupOptions(RollupBy.BLOCK, Optional.ofNullable(tz), Optional.of(blockMs));
    }

    Map<String, Object> toWire() {
        Map<String, Object> m = new LinkedHashMap<>();
        m.put("by", by.id());
        tz.ifPresent(v -> m.put("tz", v));
        blockMs.ifPresent(v -> m.put("blockMs", v));
        return m;
    }
}
