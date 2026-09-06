package com.skopli;

import com.skopli.internal.Wire;
import java.util.ArrayList;
import java.util.List;
import java.util.Map;

/** The {@code detectHarnesses} result: which registered harnesses have readable
 * data reachable from the context. {@code unsupported} has no representation
 * over this C ABI and is always empty. */
public record Detection(List<Harness> supported, List<Harness> unsupported) {

    static Detection fromWire(Map<String, Object> m) {
        return new Detection(harnesses(m.get("supported")), harnesses(m.get("unsupported")));
    }

    private static List<Harness> harnesses(Object arr) {
        List<Harness> out = new ArrayList<>();
        for (Object v : Wire.asArray(arr)) {
            out.add(Harness.fromId(Wire.asString(v)));
        }
        return List.copyOf(out);
    }
}
