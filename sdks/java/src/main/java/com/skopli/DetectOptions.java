package com.skopli;

import java.util.LinkedHashMap;
import java.util.Map;
import java.util.Optional;

/**
 * Options for {@link Skopli#detectHarnesses}. The {@code home}/{@code env}
 * path seam travels as plain data in the options JSON (facades keep whatever
 * resolver sugar is idiomatic and flatten to {home, env} before the FFI call).
 */
public record DetectOptions(Optional<String> home, Map<String, String> env) {

    /** All-defaults options (process home/env). */
    public static DetectOptions defaults() {
        return new DetectOptions(Optional.empty(), Map.of());
    }

    public static Builder builder() {
        return new Builder();
    }

    Map<String, Object> toWire() {
        Map<String, Object> m = new LinkedHashMap<>();
        home.ifPresent(v -> m.put("home", v));
        if (!env.isEmpty()) {
            m.put("env", new LinkedHashMap<Object, Object>(env));
        }
        return m;
    }

    /** Builder for {@link DetectOptions}. */
    public static final class Builder {
        private String home;
        private final Map<String, String> env = new LinkedHashMap<>();

        public Builder home(String home) {
            this.home = home;
            return this;
        }

        public Builder env(String key, String value) {
            this.env.put(key, value);
            return this;
        }

        public Builder env(Map<String, String> env) {
            this.env.putAll(env);
            return this;
        }

        public DetectOptions build() {
            return new DetectOptions(Optional.ofNullable(home), Map.copyOf(env));
        }
    }
}
