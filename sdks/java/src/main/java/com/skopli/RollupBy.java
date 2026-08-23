package com.skopli;

/** The dimension a rollup groups by. Mirrors the core's {@code RollupBy}; the
 * wire name is the lowercase id. */
public enum RollupBy {
    MODEL("model"),
    DAY("day"),
    SESSION("session"),
    HARNESS("harness"),
    WORKSPACE("workspace"),
    BLOCK("block");

    private final String id;

    RollupBy(String id) {
        this.id = id;
    }

    /** The wire id (lowercase). */
    public String id() {
        return id;
    }
}
