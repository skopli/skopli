# frozen_string_literal: true

require "json"
require "time"
require "fileutils"

module Skopli
  # Bounded fetch timeout (seconds), matching the TS reference: a stalled market
  # endpoint must never hang an otherwise local run.
  FETCH_TIMEOUT_S = 10

  # Default fetch: a stdlib GET returning the parsed JSON payload. Sets bounded
  # open/read timeouts and rejects a non-2xx status (unlike `Net::HTTP.get`,
  # which returns any body); such a failure is routed through the stale-fallback
  # path by the source. The lifecycle passes the raw payload straight into core
  # via the wire catalog grammar; the facade never reimplements a source parser.
  DEFAULT_FETCH = lambda do |url|
    require "net/http"
    require "uri"
    uri = URI(url)
    http = Net::HTTP.new(uri.host, uri.port)
    http.use_ssl = (uri.scheme == "https")
    http.open_timeout = FETCH_TIMEOUT_S
    http.read_timeout = FETCH_TIMEOUT_S
    response = http.request(Net::HTTP::Get.new(uri))
    raise "#{url} responded #{response.code}" unless response.is_a?(Net::HTTPSuccess)

    JSON.parse(response.body)
  end

  DEFAULT_TTL_MS = 60 * 60 * 1000

  OPENROUTER_URL = "https://openrouter.ai/api/v1/models"
  LITELLM_URL =
    "https://raw.githubusercontent.com/BerriAI/litellm/main/model_prices_and_context_window.json"
  MODELS_DEV_URL = "https://models.dev/api.json"

  # The context handed to a pricing source's `load`. Carries the injectable
  # `fetch` callable (`fetch.call(url) -> parsed JSON`, so a source does its own
  # HTTP entirely in Ruby; no callback crosses the FFI), the on-disk cache
  # directory, the freshness `ttl_ms` window, the `offline` / `refresh` flags,
  # and the `now_ms` clock reading for this load. A source returns a wire catalog
  # entry `{"source"=>, "fetchedAt"=>, "format"=>, "payload"=>}` (or a pre-parsed
  # `{"source"=>, "fetchedAt"=>, "prices"=>}`), or nil.
  SourceContext = Data.define(:fetch, :cache_dir, :ttl_ms, :offline, :refresh, :now_ms) do
    def initialize(fetch: DEFAULT_FETCH, cache_dir: nil, ttl_ms: DEFAULT_TTL_MS,
                   offline: false, refresh: false, now_ms: 0)
      super
    end
  end

  # A built-in market source: a wire `source` name, a core parser `format`, and
  # the URL its raw payload is fetched from. Implements the `load(context)`
  # source protocol (it resolves its own disk cache, fetches when needed,
  # validates every payload through the core parser, and returns a wire catalog
  # entry or nil). A user-supplied object with the same `load(context)` method
  # is driven identically.
  BuiltinSource = Data.define(:name, :format, :url) do
    def load(context)
      Lifecycle.load_source(
        self,
        cache_dir: context.cache_dir,
        ttl_ms: context.ttl_ms,
        offline: context.offline,
        refresh: context.refresh,
        fetch: context.fetch,
        now_ms: context.now_ms
      )
    end
  end

  # The full host-side pricing lifecycle for the direct-core Ruby facade:
  # built-in sources fetched here, a byte-compatible disk cache, TTL freshness,
  # offline, refresh, and stale-fallback. Core does matching + costing only and
  # never touches the network.
  module Lifecycle
    module_function

    def builtin_sources
      [
        BuiltinSource.new("openrouter", "openrouter", OPENROUTER_URL),
        BuiltinSource.new("litellm", "litellm", LITELLM_URL),
        BuiltinSource.new("models-dev", "modelsdev", MODELS_DEV_URL)
      ]
    end

    # The on-disk cache filename a built-in source reads and writes; the single
    # production formula, exposed for the constants contract test in golden/pricing/lifecycle.
    def cache_file_name(name)
      "pricing-#{name}.json"
    end

    # Format `now_ms` (epoch milliseconds) as ISO-8601 UTC with millisecond
    # precision, matching the TS/Rust cache format byte-for-byte.
    def iso(now_ms)
      Time.at(now_ms / 1000.0).utc.strftime("%Y-%m-%dT%H:%M:%S.%LZ")
    end

    # Read `<cache_dir>/<name>` in the byte-compatible cache format. Returns
    # `{ "fetchedAt" =>, "payload" =>, "stale" => }` or nil when absent/malformed.
    # `stale` is `age > ttl_ms` (age == ttl_ms is fresh; the shared contract).
    def load_cached(cache_dir, name, ttl_ms, now_ms)
      raw = begin
        File.read(File.join(cache_dir, name))
      rescue SystemCallError
        return nil
      end
      parsed = begin
        JSON.parse(raw)
      rescue JSON::ParserError
        return nil
      end
      return nil unless parsed.is_a?(Hash)

      fetched_at = parsed["fetchedAt"]
      return nil unless fetched_at.is_a?(String) && parsed.key?("payload")

      stamp = begin
        Time.iso8601(fetched_at)
      rescue ArgumentError
        return nil
      end
      age = now_ms - (stamp.to_f * 1000)
      { "fetchedAt" => fetched_at, "payload" => parsed["payload"], "stale" => age > ttl_ms }
    end

    # Write the cache file atomically (temp sibling, then rename). Returns the
    # ISO-8601 UTC `fetchedAt` stamped into the file.
    def store_cached(cache_dir, name, payload, now_ms)
      fetched_at = iso(now_ms)
      body = JSON.generate({ "fetchedAt" => fetched_at, "payload" => payload })
      FileUtils.mkdir_p(cache_dir)
      tmp = File.join(cache_dir, "#{name}.#{Process.pid}.#{rand(1 << 32).to_s(36)}.tmp")
      File.write(tmp, body)
      begin
        File.rename(tmp, File.join(cache_dir, name))
      rescue SystemCallError
        begin
          File.delete(tmp)
        rescue SystemCallError
          nil
        end
        raise
      end
      fetched_at
    end

    # Count parsed prices in a single raw catalog entry via the core parser.
    # Builds a throwaway native pricing handle whose options contain exactly this
    # one raw catalog (wire grammar, `builtin_sources: false`, mode `calculate`),
    # reads the catalog's `models` count, and lets the handle be GC-freed. Any
    # error from the probe build means the payload is unusable (zero). This
    # matches the TS "zero parsed prices is unusable" rule without
    # reimplementing a source parser in Ruby.
    def probe_prices(entry)
      opts = { "mode" => "calculate", "builtin_sources" => false, "catalogs" => [entry] }
      infos = begin
        native = NativePricing.new(JSON.generate(opts))
        JSON.parse(native.catalogs)
      rescue StandardError
        return 0
      end
      info = infos.find { |c| c["source"] == entry["source"] }
      info ? info["models"].to_i : 0
    end

    # Resolve one built-in source to a wire catalog entry (or nil), mirroring the
    # TS `cachedSource.load`: serve a fresh cache (or any cache when offline)
    # without fetching, otherwise fetch and cache, falling back to a stale cache
    # when the fetch fails or is skipped offline. Every cached/fetched payload is
    # validated through the core parser (#probe_prices): a payload that parses to
    # zero prices is unusable, exactly as TS treats a zero-size price map.
    def load_source(source, cache_dir:, ttl_ms:, offline:, refresh:, fetch:, now_ms:)
      cache_name = cache_file_name(source.name)
      cached = load_cached(cache_dir, cache_name, ttl_ms, now_ms)

      entry = lambda do |fetched_at, payload|
        {
          "source" => source.name,
          "fetchedAt" => fetched_at,
          "format" => source.format,
          "payload" => payload
        }
      end

      # a cache payload that probes to zero prices is unusable, same as no cache
      cached_catalog = nil
      if cached
        candidate = entry.call(cached["fetchedAt"], cached["payload"])
        cached_catalog = candidate if probe_prices(candidate).positive?
      end
      cache_fresh = cached && !cached["stale"] && !refresh && cached_catalog
      return cached_catalog if cached_catalog && (cache_fresh || offline)
      return nil if offline

      payload = begin
        fetch.call(source.url)
      rescue StandardError
        # any-age stale fallback keeps pricing available when the network is not
        return cached_catalog
      end
      # a fetched payload that probes to zero prices (or is not valid catalog
      # data) is a fetch failure: do NOT write the cache, serve the prior usable
      # cache if any, else skip the source
      return cached_catalog if probe_prices(entry.call(nil, payload)).zero?

      fetched_at = store_cached(cache_dir, cache_name, payload, now_ms)
      entry.call(fetched_at, payload)
    end
  end

  # The retained lifecycle config for a {Pricing} instance. Holds the resolved
  # sources, base native options (mode + overrides + explicit catalogs), and the
  # fetch/cache/offline/refresh/ttl/clock knobs, so a TTL-expiry reload can rerun
  # the source lifecycle and rebuild the native handle from the same inputs.
  # `refresh` is one-shot: it applies to the first load only; per-source
  # completion is tracked so a partial-failure retry does not refetch a source
  # whose refresh already completed.
  class PricingLoader
    attr_reader :ttl_ms, :clock

    def initialize(sources:, base_opts:, cache_dir:, ttl_ms:, offline:, fetch:, clock:, refresh:)
      @sources = sources
      @base_opts = base_opts
      @cache_dir = cache_dir
      @ttl_ms = ttl_ms
      @offline = offline
      @fetch = fetch
      @clock = clock
      @pending_refresh = refresh
      @refreshed = {}
    end

    # Run the source lifecycle at the current clock and build a native handle. A
    # rejected build/refresh is not memoized here; the caller only records
    # `loaded_at` when this returns, so a raised error retries next query and a
    # failed refresh stays pending.
    def build
      now_ms = @clock.call
      refreshing = @pending_refresh
      opts = @base_opts.dup
      loaded = (opts["catalogs"] || []).dup
      @sources.each do |source|
        # per-source refresh: a retry after a partial failure skips sources
        # whose refresh fetch already completed
        refresh = refreshing && !@refreshed.key?(source.object_id)
        context = SourceContext.new(
          fetch: @fetch, cache_dir: @cache_dir, ttl_ms: @ttl_ms,
          offline: @offline, refresh: refresh, now_ms: now_ms
        )
        catalog = source.load(context)
        @refreshed[source.object_id] = true if refresh
        loaded << catalog unless catalog.nil?
      end
      opts["catalogs"] = loaded unless loaded.empty?
      opts["builtin_sources"] = false
      native = Errors.translate { NativePricing.new(JSON.generate(opts)) }
      # only clear the one-shot refresh once the load COMPLETED
      @pending_refresh = false
      native
    end
  end

  # A pricing engine over a set of catalogs with a live lifecycle. Built by
  # {Skopli.create_pricing}. Retains its resolved sources, options, and a
  # `loaded_at` stamp from the injected clock; before every query, if the load
  # has aged past `ttl_ms` it reruns the source lifecycle (each source's own disk
  # cache keeps this cheap when still fresh), builds a fresh native handle, and
  # swaps it in. All native calls and the reload/swap run under one Mutex (native
  # FFI releases the GVL, so a flag check alone would be racy).
  class Pricing
    def initialize(native, loader)
      @native = native
      @loader = loader
      @ttl_ms = loader.ttl_ms
      @clock = loader.clock
      @loaded_at = loader.clock.call
      @lock = Mutex.new
    end

    # Provenance of the loaded catalogs: an Array of CatalogInfo.
    def catalogs
      raw = @lock.synchronize do
        ensure_fresh
        Errors.translate { @native.catalogs }
      end
      JSON.parse(raw).map { |c| CatalogInfo.from_wire(c) }
    end

    # Look up a model's price: a PriceHit or PriceMiss (branch on `#priced`).
    def lookup_model(model)
      raw = @lock.synchronize do
        ensure_fresh
        Errors.translate { @native.lookup_model(model.to_s) }
      end
      PriceLookup.from_wire(JSON.parse(raw))
    end

    # Cost a token bundle for `model` under this engine's catalogs. Facade-side
    # sugar over #lookup_model + Skopli.cost_usd; a miss costs 0.0.
    def price_tokens(tokens, model)
      lookup = lookup_model(model)
      return 0.0 unless lookup.is_a?(PriceHit)

      Skopli.cost_usd(tokens, lookup.price)
    end

    # Attach pricing to a set of rollups: an Array of PricedRollup.
    def price_rollups(rollups)
      payload = JSON.generate(rollups.map { |r| Wire.rollup_to_wire(r) })
      raw = @lock.synchronize do
        ensure_fresh
        Errors.translate { @native.price_rollups(payload) }
      end
      JSON.parse(raw).map { |r| PricedRollup.from_wire(r) }
    end

    # Price a batch of events grouped by a rollup dimension (`:model` / `:day` /
    # `:block` / ...): an Array of PricedEventGroup. `block_ms` sets the
    # billing-block width for `:block` (defaults to five hours); it is ignored
    # for other dimensions.
    def price_events(events, by, tz: nil, block_ms: nil)
      events_json = JSON.generate(events.map { |e| Wire.event_to_wire(e) })
      opts = { "by" => by.to_s }
      opts["tz"] = tz unless tz.nil?
      opts["blockMs"] = block_ms unless block_ms.nil?
      raw = @lock.synchronize do
        ensure_fresh
        Errors.translate { @native.price_events(events_json, JSON.generate(opts)) }
      end
      JSON.parse(raw).map { |g| PricedEventGroup.from_wire(g) }
    end

    private

    # Caller holds @lock. Reload when the in-memory load has aged past the TTL
    # window (now - loaded_at >= ttl_ms; note this differs from the disk-cache
    # `age > ttl_ms` staleness by design). A failed reload is not memoized:
    # loaded_at is only advanced when build returns.
    def ensure_fresh
      return if @loaded_at.nil?
      return if (@clock.call - @loaded_at) < @ttl_ms

      @native = @loader.build
      @loaded_at = @clock.call
    end
  end
end
