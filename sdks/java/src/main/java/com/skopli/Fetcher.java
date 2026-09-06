package com.skopli;

/**
 * The injectable HTTP getter handed to a {@link PricingSource}. All network I/O
 * lives host-side in Java; the resulting catalog crosses the FFI as plain data,
 * so no callback ever crosses the boundary.
 */
@FunctionalInterface
public interface Fetcher {

    /** Retrieve the bytes at {@code url}, or throw. */
    byte[] fetch(String url) throws Exception;
}
