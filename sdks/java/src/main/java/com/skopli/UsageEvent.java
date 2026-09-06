package com.skopli;

import com.skopli.internal.Wire;
import java.util.LinkedHashMap;
import java.util.Map;
import java.util.Optional;
import java.util.OptionalDouble;
import java.util.OptionalLong;

/**
 * A single normalized usage event. Optional fields are absent (empty) when the
 * source omits them - never present-but-null, matching the gold-file grammar.
 */
public record UsageEvent(
        String harness,
        String timestamp,
        String sessionId,
        String messageId,
        boolean turn,
        boolean subagent,
        String model,
        TokenCounts tokens,
        OptionalLong calls,
        OptionalDouble costUsd,
        Optional<String> workspace,
        Optional<String> title) {

    static UsageEvent fromWire(Map<String, Object> m) {
        return new UsageEvent(
                Wire.asString(m.get("harness")),
                Wire.asString(m.get("timestamp")),
                Wire.asString(m.get("sessionId")),
                Wire.asString(m.get("messageId")),
                Wire.asBool(m.get("turn")),
                Wire.asBool(m.get("subagent")),
                Wire.asString(m.get("model")),
                TokenCounts.fromWire(Wire.asObject(m.get("tokens"))),
                Wire.optLong(m.get("calls")),
                Wire.optDouble(m.get("costUsd")),
                Wire.optString(m.get("workspace")),
                Wire.optString(m.get("title")));
    }

    Map<String, Object> toWire() {
        Map<String, Object> m = new LinkedHashMap<>();
        m.put("harness", harness);
        m.put("timestamp", timestamp);
        m.put("sessionId", sessionId);
        m.put("messageId", messageId);
        m.put("turn", turn);
        m.put("subagent", subagent);
        m.put("model", model);
        m.put("tokens", tokens.toWire());
        calls.ifPresent(v -> m.put("calls", v));
        costUsd.ifPresent(v -> m.put("costUsd", v));
        workspace.ifPresent(v -> m.put("workspace", v));
        title.ifPresent(v -> m.put("title", v));
        return m;
    }
}
