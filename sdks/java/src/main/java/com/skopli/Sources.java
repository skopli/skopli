package com.skopli;

import com.skopli.internal.Cache;
import com.skopli.internal.Ffi;
import com.skopli.json.Json;
import java.io.IOException;
import java.net.URI;
import java.net.http.HttpClient;
import java.net.http.HttpRequest;
import java.net.http.HttpResponse;
import java.nio.charset.StandardCharsets;
import java.time.Duration;
import java.util.List;
import java.util.Optional;

/**
 * The built-in market pricing sources (mirroring {@code src/pricing/sources.ts}).
 * Each fetches a raw payload via the injectable {@link Fetcher}, caches it
 * byte-compatibly with the TypeScript cache format, and serves the cache within
 * TTL, offline, or as a stale fallback when a fetch fails. It never parses the
 * payload; the raw bytes reach the core parser through the catalog
 * {@code format} + {@code payload} path.
 */
public final class Sources {

    private Sources() {}

    /** OpenRouter endpoint. */
    public static final String OPENROUTER_URL = "https://openrouter.ai/api/v1/models";
    /** LiteLLM endpoint. */
    public static final String LITELLM_URL =
            "https://raw.githubusercontent.com/BerriAI/litellm/main/model_prices_and_context_window.json";
    /** models.dev endpoint. */
    public static final String MODELS_DEV_URL = "https://models.dev/api.json";

    static final Duration FETCH_TIMEOUT = Duration.ofSeconds(10);

    /** The on-disk cache filename a built-in source reads and writes; the single
     * production formula, exposed for the constants contract test in golden/pricing/lifecycle. */
    static String cacheFileName(String name) {
        return "pricing-" + name + ".json";
    }

    /** The default priority-ordered market sources: OpenRouter, LiteLLM,
     * models.dev. */
    public static List<PricingSource> builtin() {
        return List.of(openRouter(OPENROUTER_URL), liteLlm(LITELLM_URL), modelsDev(MODELS_DEV_URL));
    }

    /** The built-in OpenRouter market source. */
    public static PricingSource openRouter(String url) {
        return new BuiltinSource("openrouter", "openrouter", url);
    }

    /** The built-in LiteLLM market source. */
    public static PricingSource liteLlm(String url) {
        return new BuiltinSource("litellm", "litellm", url);
    }

    /** The built-in models.dev market source. */
    public static PricingSource modelsDev(String url) {
        return new BuiltinSource("models-dev", "modelsdev", url);
    }

    /** The default {@link Fetcher}: a bounded-timeout {@link HttpClient} GET so a
     * stalled endpoint never hangs an otherwise local run. */
    public static Fetcher httpFetcher() {
        return url -> {
            HttpClient client = HttpClient.newBuilder().connectTimeout(FETCH_TIMEOUT).build();
            HttpRequest request = HttpRequest.newBuilder(URI.create(url))
                    .timeout(FETCH_TIMEOUT)
                    .GET()
                    .build();
            HttpResponse<byte[]> response = client.send(request, HttpResponse.BodyHandlers.ofByteArray());
            int status = response.statusCode();
            if (status < 200 || status >= 300) {
                throw new IOException(url + " responded " + status);
            }
            return response.body();
        };
    }

    record BuiltinSource(String name, String format, String url) implements PricingSource {

        @Override
        public Optional<Catalog> load(SourceContext ctx) {
            String cacheName = cacheFileName(name);
            long now = ctx.clock().nowMillis();
            Cache.Cached cached = Cache.load(ctx.cacheDir(), cacheName, ctx.ttlMs(), now);
            Catalog fromCache = null;
            if (cached != null && Ffi.probeModelCount(name, format, cached.payload()) > 0) {
                fromCache = Catalog.raw(name, cached.fetchedAt(), format, cached.payload());
            }
            boolean cacheFresh = cached != null && !cached.stale() && !ctx.refresh();
            if (fromCache != null && (cacheFresh || ctx.offline())) {
                return Optional.of(fromCache);
            }
            if (ctx.offline()) {
                return Optional.ofNullable(fromCache);
            }
            if (ctx.fetch().isEmpty()) {
                return Optional.ofNullable(fromCache);
            }
            Object payload;
            try {
                byte[] bytes = ctx.fetch().get().fetch(url);
                payload = Json.parse(new String(bytes, StandardCharsets.UTF_8));
            } catch (Exception e) {
                return Optional.ofNullable(fromCache);
            }
            if (Ffi.probeModelCount(name, format, payload) == 0) {
                return Optional.ofNullable(fromCache);
            }
            String fetchedAt = Cache.store(ctx.cacheDir(), cacheName, payload, now);
            return Optional.of(Catalog.raw(name, fetchedAt, format, payload));
        }
    }
}
