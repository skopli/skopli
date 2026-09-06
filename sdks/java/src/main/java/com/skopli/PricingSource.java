package com.skopli;

import java.util.Optional;

/**
 * A host-language pricing source. {@link #name} identifies it; {@link #load}
 * produces a {@link Catalog} (or empty to contribute nothing) using the
 * injectable {@link Fetcher} in the context. Implementations run entirely in
 * Java (facade-side seam); the resulting catalog is handed to the core as data.
 */
public interface PricingSource {

    /** The source name (also the default catalog {@code source} if the produced
     * catalog leaves it blank). */
    String name();

    /** Produce a catalog, or {@link Optional#empty()} to contribute nothing. */
    Optional<Catalog> load(SourceContext ctx) throws Exception;
}
