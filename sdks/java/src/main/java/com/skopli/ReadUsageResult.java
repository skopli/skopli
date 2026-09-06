package com.skopli;

import com.skopli.internal.Wire;
import java.util.ArrayList;
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
        List<UsageEvent> events = new ArrayList<>();
        for (Object e : Wire.asArray(m.get("events"))) {
            events.add(UsageEvent.fromWire(Wire.asObject(e)));
        }
        List<Diagnostic> diagnostics = new ArrayList<>();
        for (Object d : Wire.asArray(m.get("diagnostics"))) {
            diagnostics.add(Diagnostic.fromWire(Wire.asObject(d)));
        }
        Map<String, List<String>> skipped = new LinkedHashMap<>();
        for (Map.Entry<String, Object> entry : Wire.asObject(m.get("skipped")).entrySet()) {
            List<String> paths = new ArrayList<>();
            for (Object p : Wire.asArray(entry.getValue())) {
                paths.add(Wire.asString(p));
            }
            skipped.put(entry.getKey(), paths);
        }
        return new ReadUsageResult(List.copyOf(events), List.copyOf(diagnostics), Map.copyOf(skipped));
    }
}
