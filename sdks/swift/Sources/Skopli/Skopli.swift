import Foundation

// The idiomatic Swift facade over the C ABI. Pure Swift: Codable models in/out,
// throws for errors, camelCase throughout. Marshalling goes through the 15-fn
// JSON surface via the Ffi bridge; nothing here touches CSkopli directly.
//
// Async conveniences run the sync primitive off the calling thread via
// withCheckedThrowingContinuation on a background queue.

/// A single shared encoder/decoder pair. JSONEncoder omits nil optionals by
/// default, matching the wire's absent-vs-null discipline.
private let encoder = JSONEncoder()
private let decoder = JSONDecoder()

private let workQueue = DispatchQueue(
    label: "com.skopli.work",
    qos: .userInitiated,
    attributes: .concurrent
)

private func encode<T: Encodable>(_ value: T) throws -> Data {
    do {
        return try encoder.encode(value)
    } catch {
        throw SkopliError.invalidArgument("encoding options: \(error)")
    }
}

private func decode<T: Decodable>(_ type: T.Type, from data: Data) throws -> T {
    do {
        return try decoder.decode(type, from: data)
    } catch {
        throw SkopliError.internalError("decoding native response: \(error)")
    }
}

/// The top-level Skopli operations. Static methods mirror the common facade
/// contract: detect/read/rollup/cost/pricing.
public enum Skopli {
    // MARK: - Meta

    /// The native ABI version integer (bumped on a breaking ABI change).
    public static var abiVersion: UInt32 { Ffi.abiVersion() }

    /// The conformance `schema_version` the native build emits.
    public static var schemaVersion: UInt32 { Ffi.schemaVersion() }

    /// The native library semver string.
    public static var version: String { Ffi.version() }

    /// The default on-disk pricing cache directory (platform-specific).
    public static func defaultCacheDir() throws -> String {
        try Ffi.defaultCacheDir()
    }

    // MARK: - Pipeline (sync)

    /// Report which registered harnesses have readable data reachable from
    /// `options`. `nil` options uses process defaults.
    public static func detectHarnesses(options: Options? = nil) throws -> Detection {
        let opts = try options.map(encode)
        let out = try Ffi.detectHarnesses(opts)
        return try decode(Detection.self, from: out)
    }

    /// Read usage across the selected harnesses, returning the
    /// `{events, diagnostics, skipped}` envelope. Malformed input never throws;
    /// it is reported in `diagnostics`/`skipped`.
    public static func readUsage(options: ReadUsageOptions = ReadUsageOptions()) throws
        -> ReadUsageResult
    {
        let out = try Ffi.readUsage(try encode(options))
        return try decode(ReadUsageResult.self, from: out)
    }

    /// Aggregate events by the option dimension. `blockMs` sets the
    /// billing-block width for `.block` (defaults to five hours) and is ignored
    /// for other dimensions.
    public static func rollup(
        events: [UsageEvent], by: RollupBy, tz: String? = nil, blockMs: Int64? = nil
    ) throws -> [Rollup] {
        let eventsData = try encode(events)
        let optsData = try encode(RollupOptions(by: by, tz: tz, blockMs: blockMs))
        let out = try Ffi.rollup(events: eventsData, opts: optsData)
        return try decode([Rollup].self, from: out)
    }

    /// Compute the USD cost of a token bundle under a flat or tiered price.
    public static func costUSD(tokens: TokenCounts, price: ModelPrice) throws -> Double {
        let tokensData = try encode(tokens)
        let priceData = try encode(price)
        return try Ffi.costUsd(tokens: tokensData, price: priceData)
    }

    /// Build a `Pricing` handle from the given options (resolves any
    /// facade-side `PricingSource`s first; see `PricingOptions`).
    public static func createPricing(options: PricingOptions = PricingOptions()) throws -> Pricing {
        try Pricing(options: options)
    }

    // MARK: - Pipeline (async conveniences)

    /// Async `readUsage`, running the sync primitive off-thread.
    public static func readUsage(options: ReadUsageOptions = ReadUsageOptions()) async throws
        -> ReadUsageResult
    {
        try await runOffThread { try readUsage(options: options) }
    }

    /// Async `createPricing`, running the sync primitive off-thread.
    public static func createPricing(options: PricingOptions = PricingOptions()) async throws
        -> Pricing
    {
        try await runOffThread { try createPricing(options: options) }
    }
}

/// Run a throwing sync closure on the background queue, bridging to async via a
/// checked continuation: the sync core runs off-thread.
func runOffThread<T: Sendable>(_ work: @escaping @Sendable () throws -> T) async throws -> T {
    try await withCheckedThrowingContinuation { continuation in
        workQueue.async {
            continuation.resume(with: Result { try work() })
        }
    }
}
