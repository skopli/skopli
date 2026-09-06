package com.skopli.internal;

import java.util.List;
import java.util.Map;
import java.util.Optional;
import java.util.OptionalDouble;
import java.util.OptionalLong;

/**
 * Coercion helpers between the {@link com.skopli.json.Json} value tree
 * (Map/List/String/Long/Double/Boolean/null) and facade record fields. Numbers
 * arrive as {@link Long} or {@link Double}; these normalize to the requested
 * Java type, treating absent/null as the empty {@link Optional}.
 */
public final class Wire {

    private Wire() {}

    @SuppressWarnings("unchecked")
    public static Map<String, Object> asObject(Object v) {
        if (v instanceof Map<?, ?> m) {
            return (Map<String, Object>) m;
        }
        throw new IllegalStateException("expected JSON object, got " + typeName(v));
    }

    @SuppressWarnings("unchecked")
    public static List<Object> asArray(Object v) {
        if (v instanceof List<?> l) {
            return (List<Object>) l;
        }
        throw new IllegalStateException("expected JSON array, got " + typeName(v));
    }

    public static String asString(Object v) {
        if (v instanceof String s) {
            return s;
        }
        throw new IllegalStateException("expected JSON string, got " + typeName(v));
    }

    public static long asLong(Object v) {
        return switch (v) {
            case null -> 0L;
            case Long l -> l;
            case Double d -> (long) (double) d;
            case Number n -> n.longValue();
            default -> throw new IllegalStateException("expected number, got " + typeName(v));
        };
    }

    public static double asDouble(Object v) {
        return switch (v) {
            case null -> 0.0;
            case Double d -> d;
            case Long l -> (double) l;
            case Number n -> n.doubleValue();
            default -> throw new IllegalStateException("expected number, got " + typeName(v));
        };
    }

    public static boolean asBool(Object v) {
        return v instanceof Boolean b && b;
    }

    public static Optional<String> optString(Object v) {
        return v == null ? Optional.empty() : Optional.of(asString(v));
    }

    public static OptionalLong optLong(Object v) {
        return v == null ? OptionalLong.empty() : OptionalLong.of(asLong(v));
    }

    public static OptionalDouble optDouble(Object v) {
        return v == null ? OptionalDouble.empty() : OptionalDouble.of(asDouble(v));
    }

    private static String typeName(Object v) {
        return v == null ? "null" : v.getClass().getSimpleName();
    }
}
