import Foundation

// Option value types, encoded to the JSON option shapes the C ABI consumes.
// `home`/`env` are the {home, env} path-resolution seam, travelling as plain
// data in the options JSON. Mirrors the Go SDK's Options / ReadUsageOptions /
// RollupOptions split (sdks/go/skopli/skopli.go).

/// The `{home, env}` path-resolution seam shared by `detectHarnesses` and
/// `readUsage`. All fields optional; the default means "process defaults". When
/// overriding, `env` must be the FULL environment (the native side does not read
/// the parent process env).
public struct Options: Codable, Sendable, Equatable {
    public var home: String?
    public var env: [String: String]?

    public init(home: String? = nil, env: [String: String]? = nil) {
        self.home = home
        self.env = env
    }
}

/// Drives `readUsage`: which harnesses, the since/until window, the timezone for
/// date bounds, and subagent handling. Encodes flat (home/env alongside the read
/// filters) to match the capi's single options object.
public struct ReadUsageOptions: Codable, Sendable, Equatable {
    public var home: String?
    public var env: [String: String]?
    /// Restrict the read to these harness ids; nil/empty means detect.
    public var harnesses: [Harness]?
    /// Inclusive lower bound (a `YYYY-MM-DD` date or an ISO timestamp).
    public var since: String?
    /// Exclusive upper bound (a `YYYY-MM-DD` date or an ISO timestamp).
    public var until: String?
    /// IANA timezone for date-only bound resolution and day bucketing (nil
    /// defaults to UTC, the hermetic default).
    public var tz: String?
    /// Set to "exclude" to drop subagent events.
    public var subagents: String?

    public init(
        home: String? = nil,
        env: [String: String]? = nil,
        harnesses: [Harness]? = nil,
        since: String? = nil,
        until: String? = nil,
        tz: String? = nil,
        subagents: String? = nil
    ) {
        self.home = home
        self.env = env
        self.harnesses = harnesses
        self.since = since
        self.until = until
        self.tz = tz
        self.subagents = subagents
    }
}

/// Selects the rollup dimension, the timezone (for `day`/`block` hour flooring),
/// and the billing-block width (for `block`). Matches the capi
/// `{by, tz?, blockMs?}` rollup/price-events options.
public struct RollupOptions: Codable, Sendable, Equatable {
    /// The default billing-block width (five hours) for `.block`.
    public static let defaultBlockMs: Int64 = 18_000_000

    public var by: RollupBy
    public var tz: String?
    /// Billing-block width in milliseconds for `.block`; nil uses `defaultBlockMs`.
    public var blockMs: Int64?

    public init(by: RollupBy, tz: String? = nil, blockMs: Int64? = nil) {
        self.by = by
        self.tz = tz
        self.blockMs = blockMs
    }
}
