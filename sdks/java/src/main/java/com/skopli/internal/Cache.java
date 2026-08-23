package com.skopli.internal;

import com.skopli.InternalException;
import com.skopli.json.Json;
import java.io.IOException;
import java.nio.charset.StandardCharsets;
import java.nio.file.Files;
import java.nio.file.Path;
import java.nio.file.StandardCopyOption;
import java.time.Instant;
import java.time.format.DateTimeFormatter;
import java.util.LinkedHashMap;
import java.util.Map;
import java.util.UUID;

/**
 * The byte-compatible pricing cache, mirroring {@code src/pricing/cache.ts}. One
 * file per source at {@code <cacheDir>/pricing-<source>.json} with body
 * {@code {"fetchedAt": "<ISO-8601 UTC>", "payload": <raw source JSON>}}. Writes
 * are atomic (temp sibling then move). A cache written by any facade is read by
 * any other.
 */
public final class Cache {

    private Cache() {}

    /** A cache read result: the raw payload, its stamp, and whether its age
     * exceeds the TTL ({@code age > ttlMs} is stale, {@code age == ttlMs} fresh). */
    public record Cached(String fetchedAt, Object payload, boolean stale) {}

    /** Read a cache file, returning {@code null} when it is absent, malformed, or
     * carries an unparseable timestamp. */
    public static Cached load(String cacheDir, String name, long ttlMs, long nowMillis) {
        Path path = Path.of(cacheDir, name);
        String raw;
        try {
            raw = Files.readString(path, StandardCharsets.UTF_8);
        } catch (IOException e) {
            return null;
        }
        Object parsed;
        try {
            parsed = Json.parse(raw);
        } catch (RuntimeException e) {
            return null;
        }
        if (!(parsed instanceof Map<?, ?>)) {
            return null;
        }
        Map<String, Object> file = Wire.asObject(parsed);
        Object fetchedAt = file.get("fetchedAt");
        Object payload = file.get("payload");
        if (!(fetchedAt instanceof String stamp) || payload == null) {
            return null;
        }
        long fetched;
        try {
            fetched = Instant.parse(stamp).toEpochMilli();
        } catch (RuntimeException e) {
            return null;
        }
        long age = nowMillis - fetched;
        return new Cached(stamp, payload, age > ttlMs);
    }

    /** Write a cache file atomically (temp sibling then move), stamped with
     * {@code nowMillis} as an ISO-8601 UTC timestamp, and return that stamp. */
    public static String store(String cacheDir, String name, Object payload, long nowMillis) {
        String fetchedAt = iso(nowMillis);
        Map<String, Object> file = new LinkedHashMap<>();
        file.put("fetchedAt", fetchedAt);
        file.put("payload", payload);
        byte[] body = Json.write(file).getBytes(StandardCharsets.UTF_8);
        Path dir = Path.of(cacheDir);
        Path target = dir.resolve(name);
        Path tmp = dir.resolve(name + "." + UUID.randomUUID() + ".tmp");
        try {
            Files.createDirectories(dir);
            Files.write(tmp, body);
            try {
                Files.move(tmp, target, StandardCopyOption.ATOMIC_MOVE, StandardCopyOption.REPLACE_EXISTING);
            } catch (IOException e) {
                Files.move(tmp, target, StandardCopyOption.REPLACE_EXISTING);
            }
        } catch (IOException e) {
            try {
                Files.deleteIfExists(tmp);
            } catch (IOException ignored) {
                // best effort
            }
            throw new InternalException("writing cache file " + target + ": " + e.getMessage());
        }
        return fetchedAt;
    }

    /** Render {@code millis} as an ISO-8601 UTC timestamp with millisecond
     * precision, matching the TS {@code Date.toISOString()} format. */
    public static String iso(long millis) {
        return DateTimeFormatter.ofPattern("yyyy-MM-dd'T'HH:mm:ss.SSS'Z'")
                .withZone(java.time.ZoneOffset.UTC)
                .format(Instant.ofEpochMilli(millis));
    }
}
