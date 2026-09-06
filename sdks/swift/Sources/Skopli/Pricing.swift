import Foundation

// The pricing handle + its facade-side source seam. The C ABI performs NO
// network I/O and has NO callback surface: custom sources and fetch injection
// live entirely in Swift, and the resulting catalog JSON is handed down as data
// with `builtinSources: false`. No callback ever crosses the FFI.

/// The costing mode.
public enum PricingMode: String, Codable, Sendable {
    /// Always compute cost from the catalog price.
    case calculate
    /// Use only the harness-recorded cost.
    case display
    /// Prefer the recorded cost, falling back to the calculated one.
    case auto
}

/// A programmatic per-model price override (the highest-priority catalog).
public struct Override: Codable, Sendable, Equatable {
    public var model: String
    public var input: Double
    public var output: Double
    public var cacheRead: Double?
    public var cacheWrite: Double?
    public var cacheWrite1h: Double?

    public init(
        model: String,
        input: Double,
        output: Double,
        cacheRead: Double? = nil,
        cacheWrite: Double? = nil,
        cacheWrite1h: Double? = nil
    ) {
        self.model = model
        self.input = input
        self.output = output
        self.cacheRead = cacheRead
        self.cacheWrite = cacheWrite
        self.cacheWrite1h = cacheWrite1h
    }
}

/// A pre-fetched pricing catalog handed to the core. Two forms are accepted
/// (mirroring the `ag_pricing_new` contract in capi pricing_opts.rs):
///   - `prices` set: an already-parsed `{model: ModelPrice}` map;
///   - `format` + `payload` set: a raw source payload the core parses
///     ("openrouter", "litellm", "modelsdev").
/// `payload` is arbitrary JSON, carried as a pre-serialized `RawJSON` so the raw
/// source bytes pass through untouched.
public struct Catalog: Codable, Sendable, Equatable {
    public var source: String
    public var fetchedAt: String?
    public var prices: [String: ModelPrice]?
    public var format: String?
    public var payload: RawJSON?

    public init(
        source: String,
        fetchedAt: String? = nil,
        prices: [String: ModelPrice]? = nil,
        format: String? = nil,
        payload: RawJSON? = nil
    ) {
        self.source = source
        self.fetchedAt = fetchedAt
        self.prices = prices
        self.format = format
        self.payload = payload
    }
}

/// A raw, already-encoded JSON fragment that round-trips verbatim through
/// Codable (used for `Catalog.payload`, whose bytes are a raw source snapshot).
public struct RawJSON: Codable, Sendable, Equatable {
    public let data: Data

    public init(_ data: Data) { self.data = data }

    public init(from decoder: Decoder) throws {
        let value = try decoder.singleValueContainer().decode(JSONAny.self)
        self.data = try JSONEncoder().encode(value)
    }

    public func encode(to encoder: Encoder) throws {
        let value = try JSONDecoder().decode(JSONAny.self, from: data)
        var container = encoder.singleValueContainer()
        try container.encode(value)
    }
}

/// Context passed to a `PricingSource.load`. It carries the injectable
/// fetch/clock seams plus the resolved cache lifecycle knobs. This is the
/// facade-side seam: all fetching/caching lives in Swift and the resulting
/// `Catalog` is handed down as data; no callback ever crosses the FFI.
public struct SourceContext: Sendable {
    /// Retrieve the bytes at a URL, or throw. May be a no-op-throwing default.
    public var fetch: @Sendable (URL) throws -> Data
    /// The clock used for cache freshness and fetch stamping.
    public var now: @Sendable () -> Date
    /// The resolved on-disk cache directory.
    public var cacheDir: String
    /// Serve cache only; a source with no cache is skipped.
    public var offline: Bool
    /// The freshness window in milliseconds; age > ttlMs is stale.
    public var ttlMs: Int64
    /// Force one fresh fetch, bypassing a fresh disk cache, with stale-cache
    /// fallback if the fetch fails.
    public var refresh: Bool

    public init(
        fetch: @escaping @Sendable (URL) throws -> Data,
        now: @escaping @Sendable () -> Date = { Date() },
        cacheDir: String,
        offline: Bool = false,
        ttlMs: Int64 = Pricing.defaultTtlMs,
        refresh: Bool = false
    ) {
        self.fetch = fetch
        self.now = now
        self.cacheDir = cacheDir
        self.offline = offline
        self.ttlMs = ttlMs
        self.refresh = refresh
    }
}

/// A host-language pricing source. `name` identifies it; `load` produces a
/// `Catalog` (or `nil` to contribute nothing) using the injectable seams in
/// `context`. Implementations run entirely in Swift.
public protocol PricingSource {
    var name: String { get }
    func load(context: SourceContext) throws -> Catalog?
}

/// Configures `createPricing`, at parity with the TS `CreatePricingOptions`.
/// `sources` are resolved facade-side (each `load` runs in Swift, fetching and
/// caching in the host language) and their catalogs merged with any explicit
/// `catalogs` and `overrides` before the FFI call. `builtinSources` is always
/// sent `false` to the C ABI (which performs no network I/O). Priority
/// highest-first: `overrides`, then `catalogs`, then the resolved `sources`
/// (default OpenRouter > LiteLLM > models.dev).
public struct PricingOptions {
    public var mode: PricingMode
    public var overrides: [Override]
    public var catalogs: [Catalog]
    /// Host-language sources resolved facade-side before the FFI call. `nil`
    /// defaults to the built-in OpenRouter, LiteLLM, and models.dev sources; an
    /// explicit empty array contributes none.
    public var sources: [PricingSource]?
    /// The on-disk cache directory; `nil` resolves to the platform default via
    /// `ag_default_cache_dir`.
    public var cacheDir: String?
    /// Serve cache only; sources with no cache are skipped.
    public var offline: Bool
    /// The cache freshness window in milliseconds; `nil` defaults to 1h.
    public var ttlMs: Int64?
    /// Force one fresh fetch per source, bypassing a fresh disk cache, with
    /// stale-cache fallback if the fetch fails.
    public var refresh: Bool
    /// The injectable getter handed to each source's `load`. Defaults to a
    /// bounded `URLSession` getter.
    public var fetch: @Sendable (URL) throws -> Data
    /// The injectable clock (for deterministic tests). Defaults to `Date()`.
    public var now: @Sendable () -> Date

    public init(
        mode: PricingMode = .calculate,
        overrides: [Override] = [],
        catalogs: [Catalog] = [],
        sources: [PricingSource]? = nil,
        cacheDir: String? = nil,
        offline: Bool = false,
        ttlMs: Int64? = nil,
        refresh: Bool = false,
        fetch: @escaping @Sendable (URL) throws -> Data = defaultFetch,
        now: @escaping @Sendable () -> Date = { Date() }
    ) {
        self.mode = mode
        self.overrides = overrides
        self.catalogs = catalogs
        self.sources = sources
        self.cacheDir = cacheDir
        self.offline = offline
        self.ttlMs = ttlMs
        self.refresh = refresh
        self.fetch = fetch
        self.now = now
    }
}

/// The JSON shape `ag_pricing_new` consumes. `builtinSources` is always false.
private struct NativePricingOptions: Encodable {
    var mode: String
    var overrides: [Override]
    var catalogs: [Catalog]
    var builtinSources: Bool
}

/// A live pricing handle wrapping a native catalog set. Thread-safe: a single
/// lock guards every native downcall, the TTL-expiry reload that swaps the
/// native handle, and `close()`, so a downcall can never overlap a free (the old
/// `requireHandle()` released the lock before the call - a use-after-free).
///
/// Catalogs are memoized only within the TTL window; a query after
/// `now - loadedAt >= ttlMs` reloads the sources through their normal cache
/// lifecycle (each source's own disk cache keeps that cheap when still fresh),
/// builds a fresh native handle, swaps it in, and frees the old one. A
/// long-lived instance therefore picks up refreshed market prices, mirroring the
/// TypeScript reference. `refresh` stays one-shot (first load only); a failed
/// reload is not memoized. Frees the native handle in `deinit` if `close()` was
/// not called.
public final class Pricing: @unchecked Sendable {
    /// The default cache freshness window (1 hour).
    public static let defaultTtlMs: Int64 = 60 * 60 * 1000

    private let lock = NSLock()

    // Retained source lifecycle state, so a query after TTL expiry can reload.
    private let baseCatalogs: [Catalog]
    private let sources: [PricingSource]
    private let mode: PricingMode
    private let overrides: [Override]
    private let context: SourceContext
    private let ttlMs: Int64

    // one-shot refresh: applies to the first load only; sources whose refresh
    // fetch already completed are not refetched on a retry after a partial fail
    // (tracked by position, since built-in sources are value types)
    private var pendingRefresh: Bool
    private var refreshedSources = Set<Int>()

    private var handle: OpaquePointer?
    // ms (from the injected clock) when the current handle's catalog load
    // completed; nil while no handle is loaded
    private var loadedAt: Int64?
    private var closed = false

    init(options: PricingOptions) throws {
        self.baseCatalogs = options.catalogs
        self.sources = options.sources ?? builtinSources()
        self.mode = options.mode
        self.overrides = options.overrides
        self.ttlMs = options.ttlMs ?? Pricing.defaultTtlMs
        self.pendingRefresh = options.refresh
        let cacheDir = try options.cacheDir ?? Skopli.defaultCacheDir()
        self.context = SourceContext(
            fetch: options.fetch,
            now: options.now,
            cacheDir: cacheDir,
            offline: options.offline,
            ttlMs: self.ttlMs,
            refresh: options.refresh
        )
        // Build the first handle eagerly so construction surfaces catalog errors.
        lock.lock()
        defer { lock.unlock() }
        try reload()
    }

    deinit {
        // Free the native handle if close() was not called.
        Ffi.pricingFree(handle)
    }

    /// Free the native handle now. Idempotent; waits for any in-flight call
    /// because it takes the same lock every downcall holds.
    public func close() {
        lock.lock()
        defer { lock.unlock() }
        if !closed {
            Ffi.pricingFree(handle)
            handle = nil
            closed = true
        }
    }

    /// The provenance of the loaded catalogs.
    public func catalogs() throws -> [CatalogInfo] {
        lock.lock()
        defer { lock.unlock() }
        let handle = try ensureFresh()
        let out = try Ffi.catalogInfo(handle)
        do {
            return try JSONDecoder().decode([CatalogInfo].self, from: out)
        } catch {
            throw SkopliError.internalError("decoding catalog info: \(error)")
        }
    }

    /// Price a set of pre-aggregated rollups, returning each rollup with its
    /// pricing lookup attached (a hit with an optional `tieredAggregate`, or a
    /// miss). The input rollups are the same shape `rollup` emits.
    public func priceRollups(rollups: [Rollup]) throws -> [PricedRollup] {
        lock.lock()
        defer { lock.unlock() }
        let handle = try ensureFresh()
        let rollupsData: Data
        do {
            rollupsData = try JSONEncoder().encode(rollups)
        } catch {
            throw SkopliError.invalidArgument("encoding rollups: \(error)")
        }
        let out = try Ffi.priceRollups(handle, rollups: rollupsData)
        do {
            return try JSONDecoder().decode([PricedRollup].self, from: out)
        } catch {
            throw SkopliError.internalError("decoding priced rollups: \(error)")
        }
    }

    /// Async `priceRollups`, running the sync primitive off-thread.
    public func priceRollups(rollups: [Rollup]) async throws -> [PricedRollup] {
        try await runOffThread { [self] in try priceRollups(rollups: rollups) }
    }

    /// Resolve a single model's price, returning a hit (`priced == true`) or a
    /// miss (`priced == false`).
    public func lookupModel(_ model: String) throws -> PriceLookup {
        lock.lock()
        defer { lock.unlock() }
        let handle = try ensureFresh()
        let out = try Ffi.lookupModel(handle, model: Data(model.utf8))
        do {
            return try JSONDecoder().decode(PriceLookup.self, from: out)
        } catch {
            throw SkopliError.internalError("decoding lookup: \(error)")
        }
    }

    /// Async `lookupModel`, running the sync primitive off-thread.
    public func lookupModel(_ model: String) async throws -> PriceLookup {
        try await runOffThread { [self] in try lookupModel(model) }
    }

    /// Price an event batch grouped by the option dimension, returning each
    /// bucket with its pricing lookup attached.
    public func priceEvents(
        events: [UsageEvent], by: RollupBy, tz: String? = nil, blockMs: Int64? = nil
    ) throws -> [PricedRollup] {
        lock.lock()
        defer { lock.unlock() }
        let handle = try ensureFresh()
        let eventsData: Data
        let optsData: Data
        do {
            eventsData = try JSONEncoder().encode(events)
            optsData = try JSONEncoder().encode(RollupOptions(by: by, tz: tz, blockMs: blockMs))
        } catch {
            throw SkopliError.invalidArgument("encoding price-events input: \(error)")
        }
        let out = try Ffi.priceEvents(handle, events: eventsData, opts: optsData)
        do {
            return try JSONDecoder().decode([PricedRollup].self, from: out)
        } catch {
            throw SkopliError.internalError("decoding priced events: \(error)")
        }
    }

    /// Async `priceEvents`, running the sync primitive off-thread.
    public func priceEvents(
        events: [UsageEvent], by: RollupBy, tz: String? = nil, blockMs: Int64? = nil
    ) async throws -> [PricedRollup] {
        try await runOffThread { [self] in
            try priceEvents(events: events, by: by, tz: tz, blockMs: blockMs)
        }
    }

    // MARK: - lifecycle (all under lock)

    /// Reload the catalogs if the current handle is expired (or absent) and
    /// return the live handle. Called with the lock held.
    private func ensureFresh() throws -> OpaquePointer {
        if closed {
            throw SkopliError.invalidArgument("pricing handle is closed")
        }
        let nowMs = Int64((context.now().timeIntervalSince1970 * 1000).rounded())
        let expired = loadedAt != nil && nowMs - loadedAt! >= ttlMs
        if handle == nil || expired {
            try reload()
        }
        return handle!
    }

    /// Reload sources, build a new native handle, swap it in, and free the old
    /// one. Called with the lock held. A failed reload leaves no handle memoized:
    /// the next query retries (and a failed refresh retry still honours refresh).
    private func reload() throws {
        let refreshing = pendingRefresh
        pendingRefresh = false
        do {
            var catalogs = baseCatalogs
            for (index, source) in sources.enumerated() {
                // per-source refresh: a retry after a partial failure skips
                // sources whose refresh fetch already completed
                let refresh = refreshing && !refreshedSources.contains(index)
                let sourceContext =
                    refresh == context.refresh
                    ? context
                    : SourceContext(
                        fetch: context.fetch, now: context.now, cacheDir: context.cacheDir,
                        offline: context.offline, ttlMs: context.ttlMs, refresh: refresh)
                let produced: Catalog?
                do {
                    produced = try source.load(context: sourceContext)
                } catch {
                    throw SkopliError.catalog("source \"\(source.name)\": \(error)")
                }
                if refresh { refreshedSources.insert(index) }
                if var catalog = produced {
                    if catalog.source.isEmpty { catalog.source = source.name }
                    catalogs.append(catalog)
                }
            }

            let native = NativePricingOptions(
                mode: mode.rawValue,
                overrides: overrides,
                catalogs: catalogs,
                builtinSources: false
            )
            let optsData: Data
            do {
                optsData = try JSONEncoder().encode(native)
            } catch {
                throw SkopliError.invalidArgument("encoding pricing options: \(error)")
            }
            let next = try Ffi.pricingNew(optsData)
            let previous = handle
            handle = next
            loadedAt = Int64((context.now().timeIntervalSince1970 * 1000).rounded())
            if let previous { Ffi.pricingFree(previous) }
        } catch {
            // A rejected load is not memoized, so the next query retries: this
            // failing query propagates the error (mirroring TS, where a rejected
            // catalogs load rejects that query's promise and the next call
            // retries). If a prior handle exists its completion stamp is left
            // untouched: it is already expired (that is why we reloaded), so the
            // next query sees expired and retries rather than serving it as a
            // memoized fresh handle. Only when there is no usable handle is
            // loadedAt cleared. A rejected refresh restores the pending refresh
            // so the retry honours it.
            if handle == nil { loadedAt = nil }
            if refreshing { pendingRefresh = true }
            throw error
        }
    }
}
