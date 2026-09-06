package com.skopli;

/**
 * The injectable clock used for cache freshness and fetch stamping. A test
 * substitutes a fixed clock for deterministic cache-lifecycle behavior; the
 * default reads the system time.
 */
@FunctionalInterface
public interface Clock {

    /** The current time in epoch milliseconds. */
    long nowMillis();

    /** The system clock. */
    static Clock system() {
        return System::currentTimeMillis;
    }

    /** A fixed clock pinned at {@code millis}. */
    static Clock fixed(long millis) {
        return () -> millis;
    }
}
