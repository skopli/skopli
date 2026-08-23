# frozen_string_literal: true

module Skopli
  # Value classes for the public data surface (`Data.define` value classes,
  # Ruby 3.2+), plus wire (de)serialization to/from the camelCase
  # JSON the native extension exchanges. Optional fields are `nil` and omitted
  # from the wire hash when nil (matching the core's skip-when-absent discipline).
  module Wire
    module_function

    # Put a key only when the value is non-nil (absent-vs-null discipline).
    def put(hash, key, value)
      hash[key] = value unless value.nil?
    end

    # Coerce an event-like (Event or Hash) into its wire hash.
    def event_to_wire(event)
      event.is_a?(UsageEvent) ? event.to_wire : stringify(event)
    end

    # Coerce a rollup-like (Rollup or Hash) into its wire hash.
    def rollup_to_wire(rollup)
      rollup.is_a?(Rollup) ? rollup.to_wire : stringify(rollup)
    end

    # Documented idiomatic Ruby field names mapped to the exact camelCase wire
    # keys the core reads. Applied by {stringify} so a Hash override or explicit
    # catalog written with snake_case (`cache_read`, `fetched_at`, ...) reaches
    # core as `cacheRead`, `fetchedAt`, ... rather than being silently dropped.
    WIRE_KEYS = {
      "cache_read" => "cacheRead",
      "cache_write" => "cacheWrite",
      "cache_write1h" => "cacheWrite1h",
      "cache_write_1h" => "cacheWrite1h",
      "tier_mode" => "tierMode",
      "fetched_at" => "fetchedAt",
      "tiered_aggregate" => "tieredAggregate"
    }.freeze

    # Recursively convert a value for the wire: Hash keys are stringified and
    # mapped through {WIRE_KEYS} (so nested `prices` and `tiers` carry the exact
    # camelCase cache-rate keys), Arrays are converted element-wise, scalars pass
    # through unchanged.
    def to_wire_value(value)
      case value
      when Hash
        value.each_with_object({}) do |(k, v), acc|
          key = k.to_s
          acc[WIRE_KEYS.fetch(key, key)] = to_wire_value(v)
        end
      when Array
        value.map { |v| to_wire_value(v) }
      else
        value
      end
    end

    # A caller may pass a Hash with symbol keys and idiomatic snake_case field
    # names; the wire is string-keyed camelCase (see {WIRE_KEYS}). Nested prices
    # and tiers are converted recursively.
    def stringify(hash)
      to_wire_value(hash)
    end
  end

  TokenCounts = Data.define(
    :input, :output, :cache_read, :cache_write, :reasoning, :cache_write1h
  ) do
    def initialize(input: 0, output: 0, cache_read: 0, cache_write: 0,
                   reasoning: 0, cache_write1h: nil)
      super
    end

    def to_wire
      obj = {
        "input" => input,
        "output" => output,
        "cacheRead" => cache_read,
        "cacheWrite" => cache_write,
        "reasoning" => reasoning
      }
      Wire.put(obj, "cacheWrite1h", cache_write1h)
      obj
    end

    def self.from_wire(d)
      cw1h = d["cacheWrite1h"]
      new(
        input: (d["input"] || 0).to_i,
        output: (d["output"] || 0).to_i,
        cache_read: (d["cacheRead"] || 0).to_i,
        cache_write: (d["cacheWrite"] || 0).to_i,
        reasoning: (d["reasoning"] || 0).to_i,
        cache_write1h: cw1h.nil? ? nil : cw1h.to_i
      )
    end
  end

  UsageEvent = Data.define(
    :harness, :timestamp, :session_id, :message_id, :turn, :subagent,
    :model, :tokens, :calls, :cost_usd, :workspace, :title
  ) do
    def initialize(harness:, timestamp:, session_id:, message_id:, turn:,
                   subagent:, model:, tokens:, calls: nil, cost_usd: nil,
                   workspace: nil, title: nil)
      super
    end

    def to_wire
      obj = {
        "harness" => harness,
        "timestamp" => timestamp,
        "sessionId" => session_id,
        "messageId" => message_id,
        "turn" => turn,
        "subagent" => subagent,
        "model" => model,
        "tokens" => tokens.to_wire
      }
      Wire.put(obj, "calls", calls)
      Wire.put(obj, "costUsd", cost_usd)
      Wire.put(obj, "workspace", workspace)
      Wire.put(obj, "title", title)
      obj
    end

    def self.from_wire(d)
      new(
        harness: d["harness"],
        timestamp: d["timestamp"],
        session_id: d["sessionId"],
        message_id: d["messageId"],
        turn: d.fetch("turn", false) ? true : false,
        subagent: d.fetch("subagent", false) ? true : false,
        model: d["model"],
        tokens: TokenCounts.from_wire(d["tokens"]),
        calls: d["calls"],
        cost_usd: d["costUsd"],
        workspace: d["workspace"],
        title: d["title"]
      )
    end
  end

  Diagnostic = Data.define(:severity, :message, :harness) do
    def initialize(severity:, message:, harness: nil)
      super
    end

    def self.from_wire(d)
      new(severity: d["severity"], message: d["message"], harness: d["harness"])
    end
  end

  ReadUsageResult = Data.define(:events, :diagnostics, :skipped) do
    def self.from_wire(d)
      new(
        events: (d["events"] || []).map { |e| UsageEvent.from_wire(e) },
        diagnostics: (d["diagnostics"] || []).map { |x| Diagnostic.from_wire(x) },
        skipped: (d["skipped"] || {}).transform_values { |v| Array(v) }
      )
    end
  end

  Detection = Data.define(:supported, :unsupported) do
    def self.from_wire(d)
      new(supported: Array(d["supported"]), unsupported: Array(d["unsupported"]))
    end
  end

  Rollup = Data.define(:key, :tokens, :events, :turns, :calls, :cost_usd) do
    def initialize(key:, tokens:, events:, turns:, calls:, cost_usd: nil)
      super
    end

    def to_wire
      obj = {
        "key" => key,
        "tokens" => tokens.to_wire,
        "events" => events,
        "turns" => turns,
        "calls" => calls
      }
      Wire.put(obj, "costUsd", cost_usd)
      obj
    end

    def self.from_wire(d)
      new(
        key: d["key"],
        tokens: TokenCounts.from_wire(d["tokens"]),
        events: d["events"].to_i,
        turns: d["turns"].to_i,
        calls: d["calls"].to_i,
        cost_usd: d["costUsd"]
      )
    end
  end

  PriceTier = Data.define(
    :threshold, :input, :output, :cache_read, :cache_write, :cache_write1h
  ) do
    def initialize(threshold:, input:, output:, cache_read: nil,
                   cache_write: nil, cache_write1h: nil)
      super
    end

    def to_wire
      obj = { "threshold" => threshold, "input" => input, "output" => output }
      Wire.put(obj, "cacheRead", cache_read)
      Wire.put(obj, "cacheWrite", cache_write)
      Wire.put(obj, "cacheWrite1h", cache_write1h)
      obj
    end

    def self.from_wire(d)
      new(
        threshold: d["threshold"].to_f,
        input: d["input"].to_f,
        output: d["output"].to_f,
        cache_read: d["cacheRead"],
        cache_write: d["cacheWrite"],
        cache_write1h: d["cacheWrite1h"]
      )
    end
  end

  ModelPrice = Data.define(
    :input, :output, :cache_read, :cache_write, :cache_write1h, :tiers, :tier_mode
  ) do
    def initialize(input:, output:, cache_read: nil, cache_write: nil,
                   cache_write1h: nil, tiers: nil, tier_mode: nil)
      super
    end

    def to_wire
      obj = { "input" => input, "output" => output }
      Wire.put(obj, "cacheRead", cache_read)
      Wire.put(obj, "cacheWrite", cache_write)
      Wire.put(obj, "cacheWrite1h", cache_write1h)
      obj["tiers"] = tiers.map(&:to_wire) unless tiers.nil?
      Wire.put(obj, "tierMode", tier_mode)
      obj
    end

    def self.from_wire(d)
      tiers = d["tiers"]
      new(
        input: d["input"].to_f,
        output: d["output"].to_f,
        cache_read: d["cacheRead"],
        cache_write: d["cacheWrite"],
        cache_write1h: d["cacheWrite1h"],
        tiers: tiers.nil? ? nil : tiers.map { |t| PriceTier.from_wire(t) },
        tier_mode: d["tierMode"]
      )
    end
  end

  # A successful price lookup. `priced` is always true (union discriminant).
  PriceHit = Data.define(:model, :key, :price, :source, :fetched_at, :tiered_aggregate) do
    def initialize(model:, key:, price:, source:, fetched_at: nil, tiered_aggregate: false)
      super
    end

    def priced = true

    def self.from_wire(d)
      new(
        model: d["model"],
        key: d["key"],
        price: ModelPrice.from_wire(d["price"]),
        source: d["source"],
        fetched_at: d["fetchedAt"],
        tiered_aggregate: d.fetch("tieredAggregate", false) ? true : false
      )
    end
  end

  # A failed price lookup. `priced` is always false (union discriminant).
  PriceMiss = Data.define(:model, :attempted, :reason, :key) do
    def initialize(model:, attempted:, reason: nil, key: nil)
      super
    end

    def priced = false

    def self.from_wire(d)
      new(
        model: d["model"],
        attempted: Array(d["attempted"]),
        reason: d["reason"],
        key: d["key"]
      )
    end
  end

  # A PriceHit or PriceMiss, discriminated by the `priced` wire flag.
  module PriceLookup
    module_function

    def from_wire(d)
      d["priced"] ? PriceHit.from_wire(d) : PriceMiss.from_wire(d)
    end
  end

  CatalogInfo = Data.define(:source, :models, :fetched_at) do
    def initialize(source:, models:, fetched_at: nil)
      super
    end

    def self.from_wire(d)
      new(source: d["source"], models: d["models"].to_i, fetched_at: d["fetchedAt"])
    end
  end

  # A rollup with an attached `pricing` lookup and derived `usd`.
  PricedRollup = Data.define(:rollup, :pricing, :usd) do
    def self.from_wire(d)
      pricing = d["pricing"]
      new(
        rollup: Rollup.from_wire(d),
        pricing: PriceLookup.from_wire(pricing),
        usd: pricing["usd"]
      )
    end
  end

  # An event-group rollup with an attached `pricing` lookup and `usd`.
  PricedEventGroup = Data.define(:rollup, :pricing, :usd) do
    def self.from_wire(d)
      pricing = d["pricing"]
      new(
        rollup: Rollup.from_wire(d),
        pricing: PriceLookup.from_wire(pricing),
        usd: pricing["usd"]
      )
    end
  end
end
