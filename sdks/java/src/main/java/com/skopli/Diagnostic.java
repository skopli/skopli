package com.skopli;

import com.skopli.internal.Wire;
import java.util.Map;
import java.util.Optional;

/** A non-fatal read-time diagnostic (e.g. a dropped event or a parse warning),
 * carried in the read envelope rather than thrown. */
public record Diagnostic(String severity, String message, Optional<String> harness) {

    static Diagnostic fromWire(Map<String, Object> m) {
        return new Diagnostic(
                Wire.asString(m.get("severity")),
                Wire.asString(m.get("message")),
                Wire.optString(m.get("harness")));
    }
}
