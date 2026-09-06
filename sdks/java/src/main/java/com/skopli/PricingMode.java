package com.skopli;

/** The costing mode for a {@link Pricing} handle. */
public enum PricingMode {
    /** Always compute cost from the catalog price. */
    CALCULATE("calculate"),
    /** Use only the harness-recorded cost. */
    DISPLAY("display"),
    /** Prefer the recorded cost, falling back to the calculated one. */
    AUTO("auto");

    private final String id;

    PricingMode(String id) {
        this.id = id;
    }

    /** The wire id. */
    public String id() {
        return id;
    }
}
