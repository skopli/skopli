package com.skopli;

import com.skopli.internal.Wire;
import java.util.LinkedHashMap;
import java.util.List;
import java.util.Map;

/** The {@code readUsage} envelope: the union of events across the selected
 * harnesses, plus non-fatal diagnostics and per-harness skipped paths. */
public record ReadUsageResult(
        List<UsageEvent> events,
        List<Diagnostic> diagnostics,
        Map<String, List<String>> skipped) {

    static ReadUsageResult fromWire(Map<String, Object> m) {
        Map<String, List<String>> skipped = new LinkedHashMap<>();
        for (Map.Entry<String, Object> entry : Wire.asObject(m.get("skipped")).entrySet()) {
            skipped.put(entry.getKey(), Wire.list(entry.getValue(), Wire::asString));
        }
        return new ReadUsageResult(
                Wire.objects(m.get("events"), UsageEvent::fromWire),
                Wire.objects(m.get("diagnostics"), Diagnostic::fromWire),
                Map.copyOf(skipped));
    }
}
