# frozen_string_literal: true

require "json"

require_relative "skopli/version"

# The native extension defines `Skopli::NativeError` (a StandardError) and
# the `Skopli::native_*` module functions + `Skopli::NativePricing`. Load
# it under the `Skopli` namespace before the pure-Ruby facade uses it.
require_relative "skopli/skopli"

require_relative "skopli/errors"
require_relative "skopli/models"
require_relative "skopli/pricing"

# Read, roll up, and price AI coding-agent token usage.
#
# A typed, idiomatic Ruby facade over the native (magnus) engine that binds
# `skopli-core`. The native layer exchanges JSON strings; this facade owns
# the idiomatic surface (`Data.define` value classes, symbol dimensions, the
# `Skopli::Error` hierarchy) and the data-level seams (path overrides
# `{home, env}` and pre-fetched pricing catalogs travel down as data; nothing
# crosses the FFI as a callback).
#
# Sync-only v1: every native entry releases the GVL, so Ruby threads make
# progress while the Rust core runs.
module Skopli
  module_function

  # The default on-disk pricing-cache directory (platform-specific layout).
  def default_cache_dir
    native_default_cache_dir
  end

  # Detect which harnesses have readable data under the given context. Returns a
  # Detection. `env` is the ENTIRE environment the readers see (see #read_usage).
  def detect_harnesses(home: nil, env: nil)
    opts = resolve_context(home, env)
    raw = Errors.translate { native_detect_harnesses(JSON.generate(opts)) }
    Detection.from_wire(JSON.parse(raw))
  end

  # Read usage across the selected harnesses.
  #
  # `since`/`until` are date (`YYYY-MM-DD`, UTC midnight) or ISO strings; `tz` is
  # an IANA timezone for day bucketing (defaults to UTC); `subagents` is
  # `:exclude` to drop subagent events. `harnesses` is an Array of harness names
  # (Strings or Symbols like `:claude`). Returns a ReadUsageResult
  # (`events`, `diagnostics`, `skipped`).
  def read_usage(home: nil, env: nil, harnesses: nil, since: nil, until_: nil,
                 tz: nil, subagents: nil)
    opts = resolve_context(home, env)
    opts["harnesses"] = harnesses.map(&:to_s) unless harnesses.nil?
    opts["since"] = since unless since.nil?
    opts["until"] = until_ unless until_.nil?
    opts["tz"] = tz unless tz.nil?
    opts["subagents"] = subagents.to_s unless subagents.nil?
    raw = Errors.translate { native_read_usage(JSON.generate(opts)) }
    ReadUsageResult.from_wire(JSON.parse(raw))
  end

  # Roll up a batch of events by a dimension (`:model` / `:day` / `:block` /
  # `:harness` / ...). Returns an Array of Rollup. `block_ms` sets the
  # billing-block width for `:block` (defaults to five hours); it is ignored for
  # other dimensions.
  def rollup(events, by, tz: nil, block_ms: nil)
    events_json = JSON.generate(events.map { |e| Wire.event_to_wire(e) })
    opts = { "by" => by.to_s }
    opts["tz"] = tz unless tz.nil?
    opts["blockMs"] = block_ms unless block_ms.nil?
    raw = Errors.translate { native_rollup(events_json, JSON.generate(opts)) }
    JSON.parse(raw).map { |r| Rollup.from_wire(r) }
  end

  # Compute the USD cost of a token bundle under a flat/tiered price. Both
  # arguments accept a value class (TokenCounts / ModelPrice) or a Hash.
  def cost_usd(tokens, price)
    tokens_json = JSON.generate(tokens.is_a?(TokenCounts) ? tokens.to_wire : Wire.stringify(tokens))
    price_json = JSON.generate(price.is_a?(ModelPrice) ? price.to_wire : Wire.stringify(price))
    Errors.translate { native_cost_usd(tokens_json, price_json) }
  end

  # Build a Pricing engine with the full built-in pricing lifecycle.
  #
  # The facade owns fetching and the on-disk cache; the native core does
  # matching + costing only and never touches the network (`builtin_sources` is
  # always sent as false on the wire, the direct-core seam contract).
  #
  # Options mirror the TS `createPricing`:
  #   - `overrides`: highest-priority `{model:, input:, output:, cache_read:?, ...}`
  #     (snake_case cache-rate fields are converted to the camelCase wire keys).
  #   - `catalogs`: pre-fetched catalogs, each `{source:, fetched_at:?, prices:}`
  #     or `{source:, fetched_at:?, format:, payload:}` (format one of
  #     "openrouter" / "litellm" / "modelsdev"); highest priority after overrides.
  #   - `sources`: priority-ordered pricing sources, resolved HERE in Ruby; each
  #     is any object with a `load(context)` method (the three built-ins,
  #     BuiltinSource, are implementations of it, and a custom source with the
  #     same method is driven identically). Defaults to
  #     OpenRouter > LiteLLM > models.dev; pass `sources: []` for no network.
  #   - `cache_dir`: on-disk cache directory (defaults to {.default_cache_dir}).
  #   - `offline`: serve from cache only, never fetch.
  #   - `ttl_ms`: cache freshness window (default 1h). Age `> ttl_ms` is stale.
  #   - `refresh`: force one fresh fetch per source, with stale-cache fallback.
  #   - `fetch`: injectable `fetch.call(url) -> parsed JSON` seam for tests.
  #
  # `mode` is `:calculate` (default) / `:display` / `:auto`. `clock` is a
  # test-side seam returning epoch milliseconds (defaults to the wall clock).
  def create_pricing(mode: nil, overrides: nil, catalogs: nil, sources: nil,
                     cache_dir: nil, offline: false, ttl_ms: nil, refresh: false,
                     fetch: nil, clock: nil)
    resolved_sources = sources.nil? ? Lifecycle.builtin_sources : sources
    resolved_cache_dir = cache_dir.nil? ? default_cache_dir : cache_dir
    resolved_ttl = ttl_ms.nil? ? DEFAULT_TTL_MS : ttl_ms
    fetch_impl = fetch.nil? ? DEFAULT_FETCH : fetch
    clock_impl = clock.nil? ? -> { Time.now.to_f * 1000 } : clock

    base_opts = {}
    base_opts["mode"] = mode.to_s unless mode.nil?
    base_opts["overrides"] = overrides.map { |o| Wire.stringify(o) } if overrides && !overrides.empty?
    base_opts["catalogs"] = catalogs.map { |c| Wire.stringify(c) } if catalogs && !catalogs.empty?

    loader = PricingLoader.new(
      sources: resolved_sources,
      base_opts: base_opts,
      cache_dir: resolved_cache_dir,
      ttl_ms: resolved_ttl,
      offline: offline,
      fetch: fetch_impl,
      clock: clock_impl,
      refresh: refresh
    )
    native = loader.build
    Pricing.new(native, loader)
  end

  # Resolve the `{home, env}` path-override seam to concrete data.
  #
  # An explicit `env` mapping is the ENTIRE environment the readers see (the
  # process environment is not consulted downstream), so when a caller omits it
  # we snapshot `ENV` here (the native side cannot see the parent process env).
  # Empty-string values are dropped.
  def resolve_context(home, env)
    ctx = {}
    resolved_home = home.nil? ? Dir.home : home
    ctx["home"] = resolved_home unless resolved_home.nil? || resolved_home.empty?
    source_env = env.nil? ? ENV.to_h : env
    ctx["env"] = source_env.each_with_object({}) do |(k, v), acc|
      acc[k.to_s] = v.to_s unless v.nil? || v.to_s.empty?
    end
    ctx
  end
end
