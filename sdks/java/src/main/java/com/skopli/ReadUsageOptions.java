package com.skopli;

import java.util.ArrayList;
import java.util.LinkedHashMap;
import java.util.List;
import java.util.Map;
import java.util.Optional;

/**
 * Options for {@link Skopli#readUsage}. Carries the {home, env} path seam
 * plus the harness allow-list and date/tz/subagent filters. {@code since}/
 * {@code until} are date strings ({@code YYYY-MM-DD} = UTC midnight, or ISO);
 * an unparseable value raises {@link InvalidArgumentException}.
 */
public record ReadUsageOptions(
        Optional<String> home,
        Map<String, String> env,
        Optional<String> cwd,
        Optional<List<Harness>> harnesses,
        Optional<String> since,
        Optional<String> until,
        Optional<String> tz,
        boolean excludeSubagents) {

    public static Builder builder() {
        return new Builder();
    }

    Map<String, Object> toWire() {
        Map<String, Object> m = new LinkedHashMap<>();
        home.ifPresent(v -> m.put("home", v));
        if (!env.isEmpty()) {
            m.put("env", new LinkedHashMap<Object, Object>(env));
        }
        cwd.ifPresent(v -> m.put("cwd", v));
        harnesses.ifPresent(hs -> {
            List<Object> arr = new ArrayList<>();
            for (Harness h : hs) {
                arr.add(h.id());
            }
            m.put("harnesses", arr);
        });
        since.ifPresent(v -> m.put("since", v));
        until.ifPresent(v -> m.put("until", v));
        tz.ifPresent(v -> m.put("tz", v));
        if (excludeSubagents) {
            m.put("subagents", "exclude");
        }
        return m;
    }

    /** Builder for {@link ReadUsageOptions}. */
    public static final class Builder {
        private String home;
        private final Map<String, String> env = new LinkedHashMap<>();
        private String cwd;
        private List<Harness> harnesses;
        private String since;
        private String until;
        private String tz;
        private boolean excludeSubagents;

        public Builder home(String home) {
            this.home = home;
            return this;
        }

        public Builder cwd(String cwd) {
            this.cwd = cwd;
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

        public Builder harnesses(List<Harness> harnesses) {
            this.harnesses = new ArrayList<>(harnesses);
            return this;
        }

        public Builder harness(Harness harness) {
            if (this.harnesses == null) {
                this.harnesses = new ArrayList<>();
            }
            this.harnesses.add(harness);
            return this;
        }

        public Builder since(String since) {
            this.since = since;
            return this;
        }

        public Builder until(String until) {
            this.until = until;
            return this;
        }

        public Builder tz(String tz) {
            this.tz = tz;
            return this;
        }

        public Builder excludeSubagents(boolean excludeSubagents) {
            this.excludeSubagents = excludeSubagents;
            return this;
        }

        public ReadUsageOptions build() {
            return new ReadUsageOptions(
                    Optional.ofNullable(home),
                    Map.copyOf(env),
                    Optional.ofNullable(cwd),
                    Optional.ofNullable(harnesses).map(List::copyOf),
                    Optional.ofNullable(since),
                    Optional.ofNullable(until),
                    Optional.ofNullable(tz),
                    excludeSubagents);
        }
    }
}
