# frozen_string_literal: true

# Ruby smoke test: read -> rollup -> price against a golden case.
#
# Exercises the typed facade end to end and compares the results structurally
# against the committed gold envelopes (golden/claude/basic): the native gem,
# compiled + loaded, driving the whole pipeline with parity against the shared
# conformance fixtures.

require "json"
require "minitest/autorun"

require "skopli"

require "tmpdir"

REPO_ROOT = File.expand_path("../../..", __dir__)
CASE_DIR = File.join(REPO_ROOT, "golden", "claude", "basic")
LIFECYCLE_DIR = File.join(REPO_ROOT, "golden", "pricing", "lifecycle")
PROBE_DIR = File.join(REPO_ROOT, "golden", "pricing", "probe")
PRICING_BASIC_DIR = File.join(REPO_ROOT, "golden", "pricing", "basic")
CATALOGS_DIR = File.join(REPO_ROOT, "golden", "pricing", "catalogs")

module GoldHelpers
  def load_gold(name)
    JSON.parse(File.read(File.join(CASE_DIR, name)))
  end

  def manifest
    load_gold("input-manifest.json")
  end

  # The manifest's env maps CLAUDE_CONFIG_DIR to "." relative to the input dir.
  def config_dir
    File.join(CASE_DIR, "input")
  end

  def read_case
    m = manifest
    Skopli.read_usage(
      home: config_dir,
      env: { "CLAUDE_CONFIG_DIR" => config_dir },
      harnesses: m["options"]["harnesses"],
      tz: m["options"]["tz"]
    )
  end
end

class SmokeTest < Minitest::Test
  include GoldHelpers

  def test_case_fixture_exists
    assert Dir.exist?(CASE_DIR), "golden case missing: #{CASE_DIR}"
  end

  def test_read_usage_matches_gold_events
    expected = load_gold("expected-events.json")["events"]
    result = read_case

    assert_instance_of Skopli::ReadUsageResult, result
    assert_equal expected.length, result.events.length

    # Structural parity: re-serialize each typed event to the wire shape and
    # compare against the gold events (order-preserving, as the reader emits).
    got_wire = result.events.map(&:to_wire)
    assert_equal expected, got_wire

    first = result.events.first
    assert_equal "claude", first.harness
    assert_instance_of Skopli::TokenCounts, first.tokens
    assert_equal expected[0]["tokens"]["input"], first.tokens.input
  end

  def test_detect_harnesses_finds_claude
    detection = Skopli.detect_harnesses(
      home: config_dir,
      env: { "CLAUDE_CONFIG_DIR" => config_dir }
    )
    assert_instance_of Skopli::Detection, detection
    assert_includes detection.supported, "claude"
  end

  def test_rollup_matches_gold
    %i[model day harness].each do |by|
      expected = load_gold("expected-rollup.json")["by"][by.to_s]
      rollups = Skopli.rollup(read_case.events, by, tz: "UTC")

      assert(rollups.all? { |r| r.is_a?(Skopli::Rollup) })

      # Strip the derived costUsd from both sides: it is a pricing-layer field
      # the bare rollup should not add, but the gold includes it where the TS
      # exporter priced. Compare token/event/turn/call structure by key.
      got = rollups.to_h { |r| [r.key, r.to_wire] }
      exp = expected.to_h { |r| [r["key"], r] }
      assert_equal exp.keys.sort, got.keys.sort

      exp.each_key do |key|
        g = got[key].reject { |k, _| k == "costUsd" }
        e = exp[key].reject { |k, _| k == "costUsd" }
        assert_equal e, g, "rollup #{by}/#{key} mismatch"
      end
    end
  end

  def test_cost_usd_flat_price
    tokens = Skopli::TokenCounts.new(input: 1000, output: 500)
    price = Skopli::ModelPrice.new(input: 1.25, output: 10.0)
    # (1000/1e6)*1.25 + (500/1e6)*10 = 0.00125 + 0.005 = 0.00625
    assert_in_delta 0.00625, Skopli.cost_usd(tokens, price), 1e-9
  end

  def test_price_rollups_hit_and_miss
    rollups = Skopli.rollup(read_case.events, :model, tz: "UTC")

    pricing = Skopli.create_pricing(
      overrides: [{ model: "claude-sonnet-4-5-20250929", input: 3.0, output: 15.0 }],
      sources: []
    )
    priced = pricing.price_rollups(rollups)
    refute_empty priced

    by_key = priced.to_h { |p| [p.rollup.key, p] }

    sonnet = by_key["claude-sonnet-4-5-20250929"]
    assert_instance_of Skopli::PriceHit, sonnet.pricing
    assert_equal "override", sonnet.pricing.source
    assert sonnet.usd && sonnet.usd > 0

    mystery = by_key["mystery-model-x"]
    assert_instance_of Skopli::PriceMiss, mystery.pricing
    refute mystery.pricing.priced
  end

  def test_price_events_grouped
    pricing = Skopli.create_pricing(
      overrides: [{ model: "claude-sonnet-4-5-20250929", input: 3.0, output: 15.0 }],
      sources: []
    )
    groups = pricing.price_events(read_case.events, :day, tz: "UTC")
    refute_empty groups
    keys = groups.map { |g| g.rollup.key }
    assert_includes keys, "2026-08-01"
  end

  def test_lookup_model_union
    pricing = Skopli.create_pricing(
      overrides: [{ model: "known", input: 1.0, output: 2.0 }],
      sources: []
    )
    hit = pricing.lookup_model("known")
    assert_instance_of Skopli::PriceHit, hit
    assert_equal "override", hit.source

    miss = pricing.lookup_model("unknown-xyz")
    assert_instance_of Skopli::PriceMiss, miss
  end

  def test_invalid_argument_raises
    assert_raises(Skopli::InvalidArgumentError) do
      Skopli.rollup([], :"not-a-dimension")
    end
    # InvalidArgumentError is also an ArgumentError (idiom parity).
    assert_raises(ArgumentError) do
      Skopli.rollup([], :"not-a-dimension")
    end
  end

  def test_default_cache_dir_nonempty
    refute_empty Skopli.default_cache_dir
  end
end

module LifecycleHelpers
  def lifecycle(name)
    JSON.parse(File.read(File.join(LIFECYCLE_DIR, name)))
  end

  def iso_ms(iso)
    require "time"
    Time.iso8601(iso).to_f * 1000
  end

  def read_expected(behavior)
    expected = lifecycle(File.join("expected", "#{behavior}.json"))
    assert_equal behavior, expected["behavior"]
    expected
  end

  # Select the built-in source named by request.source and load its canonical
  # source-<name>.json as the successful fetch body (never derive it from a
  # cache fixture payload). This is what the README pins as the fetch response.
  def builtin_source(request)
    name = request["source"]
    source = Skopli::BuiltinSource.new(name, name, "https://#{name}.test/api")
    [source, lifecycle("source-#{name}.json")]
  end

  # A recording fetch: counts calls, then serves the payload (or raises when
  # `throws`, after incrementing so the attempt is observable).
  def recording_fetch(payload, throws: false)
    calls = { n: 0 }
    fetch = lambda do |_url|
      calls[:n] += 1
      raise "network down" if throws

      payload
    end
    [fetch, calls]
  end

  def run_behavior(expected, install: nil, offline: false, refresh: false, throws: false)
    # Drive one lifecycle behavior against the committed fixtures and consume
    # every key of expected/<behavior>.json: behavior (via read_expected), exact
    # fetch count, priced, pricedUsd, and the exact fetchedAt (or its absence).
    # The source and fetch body come from request.source, never a hard-coded
    # name or a cache-derived payload.
    request = lifecycle("request.json")
    source, source_payload = builtin_source(request)
    Dir.mktmpdir do |dir|
      cache_file = File.join(dir, request["cacheFileName"])
      File.write(cache_file, JSON.generate(lifecycle(install))) if install
      now_ms = iso_ms(request["now"])
      fetch, calls = recording_fetch(source_payload, throws: throws)
      pricing = Skopli.create_pricing(
        mode: request["options"]["mode"],
        sources: [source],
        cache_dir: dir,
        ttl_ms: request["ttlMs"],
        offline: offline,
        refresh: refresh,
        fetch: fetch,
        clock: -> { now_ms }
      )
      priced = pricing.price_rollups([Skopli::Rollup.from_wire(request["rollup"])])
      pr = priced.first

      assert_equal(expected["fetched"] ? 1 : 0, calls[:n])
      if expected["priced"]
        assert_instance_of Skopli::PriceHit, pr.pricing
        assert_in_delta expected["pricedUsd"], pr.usd, 1e-10
      else
        assert_instance_of Skopli::PriceMiss, pr.pricing
        assert_equal expected["pricedUsd"], (pr.usd || 0)
      end

      if expected["fetchedAt"].nil?
        refute File.exist?(cache_file)
      else
        body = JSON.parse(File.read(cache_file))
        assert_equal expected["fetchedAt"], body["fetchedAt"]
      end
    end
  end
end

class LifecycleConformanceTest < Minitest::Test
  include LifecycleHelpers

  def test_cold_fetch
    run_behavior(read_expected("cold-fetch"))
  end

  def test_warm_cache
    run_behavior(read_expected("warm-cache"), install: "cache-fresh.json")
  end

  def test_ttl_refresh
    run_behavior(read_expected("ttl-refresh"), install: "cache-stale.json", refresh: true)
  end

  def test_offline
    run_behavior(read_expected("offline"), install: "cache-stale.json", offline: true)
  end

  def test_offline_no_cache
    run_behavior(read_expected("offline-no-cache"), offline: true)
  end

  def test_stale_fallback
    run_behavior(read_expected("stale-fallback"), install: "cache-stale.json", throws: true)
  end

  def test_fetched_empty
    # a stale cache is present; the fetch succeeds but returns a payload that
    # parses to zero prices. The unusable payload is discarded like a failure:
    # the stale catalog is served, the cache file is NOT overwritten.
    expected = read_expected("fetched-empty")
    request = lifecycle("request.json")
    source, = builtin_source(request)
    empty_payload = lifecycle("source-openrouter-empty.json")
    Dir.mktmpdir do |dir|
      cache_file = File.join(dir, request["cacheFileName"])
      File.write(cache_file, JSON.generate(lifecycle("cache-stale.json")))
      now_ms = iso_ms(request["now"])
      fetch, calls = recording_fetch(empty_payload)
      pricing = Skopli.create_pricing(
        mode: request["options"]["mode"],
        sources: [source],
        cache_dir: dir, ttl_ms: request["ttlMs"], fetch: fetch, clock: -> { now_ms }
      )
      priced = pricing.price_rollups([Skopli::Rollup.from_wire(request["rollup"])])
      assert_equal(expected["fetched"] ? 1 : 0, calls[:n])
      if expected["priced"]
        assert_instance_of Skopli::PriceHit, priced.first.pricing
      else
        assert_instance_of Skopli::PriceMiss, priced.first.pricing
      end
      assert_in_delta expected["pricedUsd"], priced.first.usd, 1e-10
      body = JSON.parse(File.read(cache_file))
      assert_equal expected["fetchedAt"], body["fetchedAt"]
      assert_equal request["fetchedAtStale"], body["fetchedAt"]
    end
  end

  def test_cached_empty
    # a fresh-aged cache whose payload parses to zero prices is unusable, same
    # as no cache: a fetch is issued, returns the canonical payload, the cache
    # is rewritten with fetchedAt = now, and the rollup prices to gold.
    expected = read_expected("cached-empty")
    request = lifecycle("request.json")
    source, good_payload = builtin_source(request)
    Dir.mktmpdir do |dir|
      cache_file = File.join(dir, request["cacheFileName"])
      File.write(cache_file, JSON.generate(lifecycle("cache-fresh-empty.json")))
      now_ms = iso_ms(request["now"])
      fetch, calls = recording_fetch(good_payload)
      pricing = Skopli.create_pricing(
        mode: request["options"]["mode"],
        sources: [source],
        cache_dir: dir, ttl_ms: request["ttlMs"], fetch: fetch, clock: -> { now_ms }
      )
      priced = pricing.price_rollups([Skopli::Rollup.from_wire(request["rollup"])])
      assert_equal(expected["fetched"] ? 1 : 0, calls[:n])
      if expected["priced"]
        assert_instance_of Skopli::PriceHit, priced.first.pricing
      else
        assert_instance_of Skopli::PriceMiss, priced.first.pricing
      end
      assert_in_delta expected["pricedUsd"], priced.first.usd, 1e-10
      body = JSON.parse(File.read(cache_file))
      assert_equal expected["fetchedAt"], body["fetchedAt"]
      assert_equal request["fetchedAtFetched"], body["fetchedAt"]
    end
  end

  def test_live_reload
    # one long-lived instance, empty cacheDir. Query 1 at `now` fetches and
    # prices; the clock then advances past the TTL window, so query 2 on the
    # SAME instance consults sources again (the disk cache is now stale) and a
    # second fetch is issued. Total fetches = fetchesTotal (2).
    # live-reload pins a different key set from the single-query behaviors, so
    # it gets its own consuming read: behavior, fetchesTotal, fetchedAtFirst,
    # fetchedAtSecond, priced, pricedUsd are all read and asserted below.
    expected = lifecycle(File.join("expected", "live-reload.json"))
    assert_equal "live-reload", expected["behavior"]
    request = lifecycle("request.json")
    source, good_payload = builtin_source(request)
    Dir.mktmpdir do |dir|
      cache_file = File.join(dir, request["cacheFileName"])
      now = { ms: iso_ms(request["now"]) }
      fetch, calls = recording_fetch(good_payload)
      pricing = Skopli.create_pricing(
        mode: request["options"]["mode"],
        sources: [source],
        cache_dir: dir, ttl_ms: request["ttlMs"], fetch: fetch, clock: -> { now[:ms] }
      )
      first = pricing.price_rollups([Skopli::Rollup.from_wire(request["rollup"])])
      if expected["priced"]
        assert_instance_of Skopli::PriceHit, first.first.pricing
      else
        assert_instance_of Skopli::PriceMiss, first.first.pricing
      end
      assert_in_delta expected["pricedUsd"], first.first.usd, 1e-10
      assert_equal expected["fetchedAtFirst"], JSON.parse(File.read(cache_file))["fetchedAt"]

      now[:ms] = iso_ms(request["nowSecond"])
      second = pricing.price_rollups([Skopli::Rollup.from_wire(request["rollup"])])
      if expected["priced"]
        assert_instance_of Skopli::PriceHit, second.first.pricing
      else
        assert_instance_of Skopli::PriceMiss, second.first.pricing
      end
      assert_in_delta expected["pricedUsd"], second.first.usd, 1e-10
      assert_equal expected["fetchesTotal"], calls[:n]
      assert_equal expected["fetchedAtSecond"], JSON.parse(File.read(cache_file))["fetchedAt"]
    end
  end

  def test_source_fixtures_price_to_gold
    # Each built-in source has a distinct public name and a parser format: the
    # public identity for models.dev is `models-dev` (not `modelsdev`), so its
    # shared cache filename is pricing-models-dev.json. Assert the fetch fires
    # exactly once, the catalog provenance reports the exact source name and a
    # fetchedAt of now, the exact cache file lands on disk, and the price is 2.25.
    request = lifecycle("request.json")
    [
      ["source-openrouter.json", "openrouter", "openrouter", "pricing-openrouter.json"],
      ["source-litellm.json", "litellm", "litellm", "pricing-litellm.json"],
      ["source-modelsdev.json", "models-dev", "modelsdev", "pricing-models-dev.json"]
    ].each do |file, name, fmt, cache_file|
      payload = lifecycle(file)
      Dir.mktmpdir do |dir|
        fetch, calls = recording_fetch(payload)
        now_ms = iso_ms(request["now"])
        pricing = Skopli.create_pricing(
          mode: request["options"]["mode"],
          sources: [Skopli::BuiltinSource.new(name, fmt, "https://#{name}.test/api")],
          cache_dir: dir,
          ttl_ms: request["ttlMs"],
          fetch: fetch,
          clock: -> { now_ms }
        )
        priced = pricing.price_rollups([Skopli::Rollup.from_wire(request["rollup"])])
        assert_equal 1, calls[:n], name
        assert_instance_of Skopli::PriceHit, priced.first.pricing, name
        assert_in_delta 2.25, priced.first.usd, 1e-10, name

        infos = pricing.catalogs.to_h { |c| [c.source, c] }
        assert_includes infos.keys, name
        assert_equal request["fetchedAtFetched"], infos[name].fetched_at, name

        assert File.exist?(File.join(dir, cache_file)), cache_file
      end
    end
  end

  def test_cache_interop_written_file_parses_fresh
    request = lifecycle("request.json")
    payload = lifecycle("cache-fresh.json")["payload"]
    now_ms = iso_ms(request["now"])
    Dir.mktmpdir do |dir|
      fetched_at = Skopli::Lifecycle.store_cached(dir, request["cacheFileName"], payload, now_ms)
      assert_equal request["fetchedAtFetched"], fetched_at

      body = JSON.parse(File.read(File.join(dir, request["cacheFileName"])))
      assert_equal({ "fetchedAt" => request["fetchedAtFetched"], "payload" => payload }, body)

      cached = Skopli::Lifecycle.load_cached(dir, request["cacheFileName"], request["ttlMs"], now_ms)
      refute_nil cached
      assert_equal false, cached["stale"]
      assert_equal request["fetchedAtFetched"], cached["fetchedAt"]
    end
  end
end

class LifecycleConstantsTest < Minitest::Test
  def lifecycle(name)
    JSON.parse(File.read(File.join(LIFECYCLE_DIR, name)))
  end

  def test_constants_match_golden
    # the shared golden constants contract (constants.json): the facade's own
    # source names, formats, URLs, priority order, TTL default, fetch timeout,
    # and cache filenames must equal the golden values so no facade's literals
    # drift.
    constants = lifecycle("constants.json")

    url_by_name = {
      "openrouter" => Skopli::OPENROUTER_URL,
      "litellm" => Skopli::LITELLM_URL,
      "models-dev" => Skopli::MODELS_DEV_URL
    }

    builtin = Skopli::Lifecycle.builtin_sources
    assert_equal constants["priorityOrder"], builtin.map(&:name)
    assert_equal constants["sources"].map { |s| s["name"] }, constants["priorityOrder"]

    by_name = builtin.to_h { |s| [s.name, s] }
    constants["sources"].each do |spec|
      name = spec["name"]
      source = by_name[name]
      assert_equal spec["format"], source.format, name
      assert_equal spec["url"], source.url, name
      assert_equal spec["url"], url_by_name[name], name
      # compare the production filename formula (Lifecycle.cache_file_name), not
      # just the fixture pattern, so the real formula cannot drift unseen
      assert_equal spec["cacheFileName"], Skopli::Lifecycle.cache_file_name(name), name
      assert_equal constants["cacheFileNamePattern"].sub("{name}", name), spec["cacheFileName"], name
    end

    assert_equal constants["defaultTtlMs"], Skopli::DEFAULT_TTL_MS
    # the facade keeps the fetch timeout in seconds; the golden value is in ms
    assert_equal constants["fetchTimeoutMs"], (Skopli::FETCH_TIMEOUT_S * 1000).to_i
  end
end

class ProbeConformanceTest < Minitest::Test
  # the shared unusable-payload probe contract (probe/cases.json): the facade's
  # own probe (Lifecycle.probe_prices, the same internal function its lifecycle
  # uses) must report usable/unusable exactly per case. An invalid-JSON
  # payloadRaw is fed as the raw payload value; the native build parses it to
  # zero.
  JSON.parse(File.read(File.join(PROBE_DIR, "cases.json"))).each do |kase|
    define_method("test_probe_#{kase["name"]}") do
      payload = kase.key?("payloadRaw") ? kase["payloadRaw"] : kase["payload"]
      entry = { "source" => "probe", "fetchedAt" => nil, "format" => kase["format"], "payload" => payload }
      assert_equal kase["usable"], Skopli::Lifecycle.probe_prices(entry).positive?, kase["name"]
    end
  end
end

ROLLUP_TZ_DIR = File.join(REPO_ROOT, "golden", "rollup-tz")

class RollupTzConformanceTest < Minitest::Test
  # the shared timezone day-bucketing contract (rollup-tz/cases.json): one
  # synthetic event per timestamp, rolled up by day in the case's zone, must
  # produce exactly the expected buckets. This reaches the same native core the
  # Rust and other-language suites exercise.
  JSON.parse(File.read(File.join(ROLLUP_TZ_DIR, "cases.json"))).each do |kase|
    define_method("test_rollup_tz_#{kase["name"]}") do
      events = kase["timestamps"].map do |ts|
        { "harness" => "h", "timestamp" => ts, "sessionId" => "s", "messageId" => "m",
          "turn" => false, "subagent" => false, "model" => "m",
          "tokens" => { "input" => 0, "output" => 0, "cacheRead" => 0, "cacheWrite" => 0, "reasoning" => 0 } }
      end
      rollups = Skopli.rollup(events, :day, tz: kase["tz"])
      got = rollups.map { |r| { "key" => r.key, "events" => r.events } }
      assert_equal kase["expected"], got, kase["name"]
    end
  end
end

ROLLUP_BLOCK_DIR = File.join(REPO_ROOT, "golden", "rollup-block")

class RollupBlockConformanceTest < Minitest::Test
  # the shared billing-block windowing contract (rollup-block/cases.json): one
  # synthetic event per timestamp, rolled up into blocks of the case's width in
  # the case's zone, must produce exactly the expected blocks. This reaches the
  # same native core the Rust and other-language suites exercise.
  JSON.parse(File.read(File.join(ROLLUP_BLOCK_DIR, "cases.json"))).each do |kase|
    define_method("test_rollup_block_#{kase["name"]}") do
      events = kase["timestamps"].map do |ts|
        { "harness" => "h", "timestamp" => ts, "sessionId" => "s", "messageId" => "m",
          "turn" => false, "subagent" => false, "model" => "m",
          "tokens" => { "input" => 0, "output" => 0, "cacheRead" => 0, "cacheWrite" => 0, "reasoning" => 0 } }
      end
      rollups = Skopli.rollup(events, :block, tz: kase["tz"], block_ms: kase["blockMs"])
      got = rollups.map { |r| { "key" => r.key, "events" => r.events } }
      assert_equal kase["expected"], got, kase["name"]
    end
  end

  # omitting block_ms must apply the five-hour default across the wire: two
  # events 3h41m apart join one block anchored to 09:00, and an event past five
  # hours opens a new block. Drives the production rollup with no width so the
  # facade's absent-optional serialization is exercised, not the fixture value.
  def events_for(*timestamps)
    timestamps.map do |ts|
      { "harness" => "h", "timestamp" => ts, "sessionId" => "s", "messageId" => "m",
        "turn" => false, "subagent" => false, "model" => "m",
        "tokens" => { "input" => 0, "output" => 0, "cacheRead" => 0, "cacheWrite" => 0, "reasoning" => 0 } }
    end
  end

  def test_rollup_block_omitted_width_uses_default
    joined = Skopli.rollup(
      events_for("2026-01-01T09:17:00.000Z", "2026-01-01T13:00:00.000Z"), :block, tz: "UTC"
    )
    assert_equal(
      [{ "key" => "2026-01-01T09:00:00.000Z", "events" => 2 }],
      joined.map { |r| { "key" => r.key, "events" => r.events } }
    )

    split = Skopli.rollup(
      events_for("2026-01-01T09:00:00.000Z", "2026-01-01T14:30:00.000Z"), :block, tz: "UTC"
    )
    assert_equal(
      [{ "key" => "2026-01-01T09:00:00.000Z", "events" => 1 },
       { "key" => "2026-01-01T14:00:00.000Z", "events" => 1 }],
      split.map { |r| { "key" => r.key, "events" => r.events } }
    )
  end
end

TIMESTAMPS_DIR = File.join(REPO_ROOT, "golden", "pricing", "timestamps")

class TimestampBoundaryTest < Minitest::Test
  # the shared cache fetchedAt boundary contract (timestamps/cases.json): a stamp
  # is read through the facade's own cache-read path (Lifecycle.load_cached, the
  # same it uses in production). An invalid stamp yields nil; a valid stamp
  # resolves to its epoch, pinned to the millisecond by the fresh/stale boundary
  # at now == epochMs and now == epochMs + 1 with a zero TTL.
  cases = JSON.parse(File.read(File.join(TIMESTAMPS_DIR, "cases.json")))

  cases["read"].each do |kase|
    define_method("test_read_#{kase["name"]}") do
      name = "pricing-openrouter.json"
      Dir.mktmpdir do |dir|
        File.write(File.join(dir, name), JSON.generate({ "fetchedAt" => kase["stamp"], "payload" => {} }))
        if kase["epochMs"].nil?
          assert_nil Skopli::Lifecycle.load_cached(dir, name, 0, 0), kase["name"]
          next
        end
        fresh = Skopli::Lifecycle.load_cached(dir, name, 0, kase["epochMs"])
        stale = Skopli::Lifecycle.load_cached(dir, name, 0, kase["epochMs"] + 1)
        refute_nil fresh, kase["name"]
        assert_equal false, fresh["stale"], kase["name"]
        refute_nil stale, kase["name"]
        assert_equal true, stale["stale"], kase["name"]
      end
    end
  end

  cases["write"].each do |ms|
    define_method("test_write_#{ms}") do
      name = "pricing-openrouter.json"
      Dir.mktmpdir do |dir|
        stamp = Skopli::Lifecycle.store_cached(dir, name, {}, ms)
        assert_equal 24, stamp.length, stamp
        assert stamp.end_with?("Z"), stamp
        fresh = Skopli::Lifecycle.load_cached(dir, name, 0, ms)
        stale = Skopli::Lifecycle.load_cached(dir, name, 0, ms + 1)
        assert_equal false, fresh["stale"], stamp
        assert_equal true, stale["stale"], stamp
      end
    end
  end
end

class PricingGoldTest < Minitest::Test
  def read_gold(dir, name)
    JSON.parse(File.read(File.join(dir, name)))
  end

  # Reconstruct a public typed lookup back to its wire shape so the gold
  # comparison exercises the facade round-trip for both hit and miss shapes.
  def price_lookup_to_wire(pricing, usd)
    if pricing.is_a?(Skopli::PriceHit)
      out = { "priced" => true, "model" => pricing.model, "key" => pricing.key,
              "price" => pricing.price.to_wire, "source" => pricing.source }
      out["fetchedAt"] = pricing.fetched_at unless pricing.fetched_at.nil?
      out["tieredAggregate"] = true if pricing.tiered_aggregate
      out["usd"] = usd unless usd.nil?
      out
    else
      out = { "priced" => false, "model" => pricing.model, "attempted" => pricing.attempted }
      out["reason"] = pricing.reason unless pricing.reason.nil?
      out["key"] = pricing.key unless pricing.key.nil?
      out["usd"] = usd unless usd.nil?
      out
    end
  end

  # Canonicalize key order / number formatting for a raw-JSON-tree compare
  # (round-trip through JSON with sorted keys), without altering structure.
  def canonical(value)
    case value
    when Hash then value.keys.sort.to_h { |k| [k, canonical(value[k])] }
    when Array then value.map { |v| canonical(v) }
    else value
    end
  end

  def test_price_rollups_full_gold
    gold = read_gold(PRICING_BASIC_DIR, "expected-priced-rollup.json")
    expected = gold["rollups"]
    pinned = "2026-08-01T00:00:00.000Z"

    # raw JSON copy of the expected rollups with ONLY `pricing` removed
    input_rollups = expected.map do |r|
      r = r.dup
      r.delete("pricing")
      r
    end

    catalogs = [
      { "source" => "openrouter", "fetchedAt" => pinned, "format" => "openrouter",
        "payload" => read_gold(CATALOGS_DIR, "openrouter.json") },
      { "source" => "litellm", "fetchedAt" => pinned, "format" => "litellm",
        "payload" => read_gold(CATALOGS_DIR, "litellm.json") }
    ]
    pricing = Skopli.create_pricing(mode: :calculate, catalogs: catalogs, sources: [])

    # drive the PUBLIC typed facade, then re-encode the returned
    # values back to a raw JSON tree and compare the COMPLETE tree against the
    # untouched expected rollups (canonicalized key order / number formatting)
    priced = pricing.price_rollups(input_rollups.map { |r| Skopli::Rollup.from_wire(r) })
    got = priced.map do |p|
      wire = p.rollup.to_wire
      wire["pricing"] = price_lookup_to_wire(p.pricing, p.usd)
      wire
    end
    assert_equal canonical(expected), canonical(got)
  end

  def test_overrides_carry_cache_rates_through_snake_case
    # an override written with idiomatic snake_case cache-rate fields
    # must reach core as the exact camelCase wire keys. A cache-read/cache-write
    # heavy bundle produces a different cost than the flat input/output rate, so
    # a dropped `cache_read` would silently change the result.
    pricing = Skopli.create_pricing(
      overrides: [{ model: "cache-model", input: 10.0, output: 20.0,
                    cache_read: 1.0, cache_write: 5.0 }],
      sources: []
    )
    hit = pricing.lookup_model("cache-model")
    assert_instance_of Skopli::PriceHit, hit
    assert_equal 1.0, hit.price.cache_read
    assert_equal 5.0, hit.price.cache_write

    tokens = Skopli::TokenCounts.new(input: 0, output: 0, cache_read: 1_000_000, cache_write: 1_000_000)
    # (1e6 * 1.0 + 1e6 * 5.0) / 1e6 = 6.0; had cache_read/cache_write been
    # dropped, both would fall back to the input rate (10.0) => 20.0
    assert_in_delta 6.0, pricing.price_tokens(tokens, "cache-model"), 1e-9
  end

  def test_lookup_model_hit_and_miss
    pinned = "2026-08-01T00:00:00.000Z"
    catalogs = [
      { "source" => "litellm", "fetchedAt" => pinned, "format" => "litellm",
        "payload" => read_gold(CATALOGS_DIR, "litellm.json") }
    ]
    pricing = Skopli.create_pricing(mode: :calculate, catalogs: catalogs, sources: [])

    hit = pricing.lookup_model("gpt-5")
    assert_instance_of Skopli::PriceHit, hit
    assert hit.priced
    assert_equal "litellm", hit.source

    miss = pricing.lookup_model("totally-unknown-model-9000")
    assert_instance_of Skopli::PriceMiss, miss
    refute miss.priced
  end
end
