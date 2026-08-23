import Foundation

// Codable data models mirroring the C ABI's JSON wire shapes exactly (camelCase
// keys, absent-vs-null discipline). These are the exact shapes produced/consumed
// by the capi (wire.rs / pipeline.rs) and mirrored 1:1 from the Go SDK types
// (sdks/go/skopli/types.go). Optional fields are Swift optionals so they
// round-trip the wire's absent-vs-present distinction; JSONEncoder omits nil
// optionals by default, matching the core's `omitempty`/skip discipline.

/// The identifier of an agent harness (a string enum on the wire). Unknown ids
/// from a newer core decode to `other` rather than throwing, so an older facade
/// cannot break (matching Java's `Harness.fromId` -> `OTHER` semantics).
public enum Harness: String, Codable, Sendable, Hashable, CaseIterable {
    case claude
    case codex
    case gemini
    case opencode
    case mimocode
    case commandcode
    case copilot
    case amp
    case droid
    case qwen
    case pi
    case omp
    case prime
    case gajae
    case kimchi
    case grok
    case augment
    case codebuff
    case codebuddy
    case jcode
    case mux
    case zcode
    case roo
    case cline
    case kilo
    case kilocode
    case openclaw
    case kimi
    case junie
    case devin
    case hermes
    case goose
    case zed
    case cherrystudio
    case opencodereview
    case trae
    case deepseek
    case reasonix
    case kiro
    case fx
    case other

    public init(from decoder: any Decoder) throws {
        let raw = try decoder.singleValueContainer().decode(String.self)
        self = Harness(rawValue: raw) ?? .other
    }
}

/// A rollup grouping dimension.
public enum RollupBy: String, Codable, Sendable, Hashable {
    case model
    case day
    case session
    case harness
    case workspace
    case block
}

/// A bundle of token counts for one event or aggregated bucket. Optional
/// `cacheWrite1h` is omitted when absent (matching the core's absent-vs-null
/// discipline).
public struct TokenCounts: Codable, Sendable, Equatable {
    public var input: UInt64
    public var output: UInt64
    public var cacheRead: UInt64
    public var cacheWrite: UInt64
    public var cacheWrite1h: UInt64?
    public var reasoning: UInt64

    public init(
        input: UInt64 = 0,
        output: UInt64 = 0,
        cacheRead: UInt64 = 0,
        cacheWrite: UInt64 = 0,
        cacheWrite1h: UInt64? = nil,
        reasoning: UInt64 = 0
    ) {
        self.input = input
        self.output = output
        self.cacheRead = cacheRead
        self.cacheWrite = cacheWrite
        self.cacheWrite1h = cacheWrite1h
        self.reasoning = reasoning
    }
}

/// A single usage record read from a harness. Optional fields round-trip the
/// wire's absent-vs-present distinction.
public struct UsageEvent: Codable, Sendable, Equatable {
    public var harness: Harness
    public var timestamp: String
    public var sessionId: String
    public var messageId: String
    public var turn: Bool
    public var subagent: Bool
    public var model: String
    public var tokens: TokenCounts
    public var calls: UInt64?
    public var costUsd: Double?
    public var workspace: String?
    public var title: String?

    public init(
        harness: Harness,
        timestamp: String,
        sessionId: String,
        messageId: String,
        turn: Bool,
        subagent: Bool = false,
        model: String,
        tokens: TokenCounts,
        calls: UInt64? = nil,
        costUsd: Double? = nil,
        workspace: String? = nil,
        title: String? = nil
    ) {
        self.harness = harness
        self.timestamp = timestamp
        self.sessionId = sessionId
        self.messageId = messageId
        self.turn = turn
        self.subagent = subagent
        self.model = model
        self.tokens = tokens
        self.calls = calls
        self.costUsd = costUsd
        self.workspace = workspace
        self.title = title
    }
}

/// A non-fatal warning surfaced during a read (malformed data never throws; it
/// is reported here instead).
public struct Diagnostic: Codable, Sendable, Equatable {
    public var severity: String
    public var message: String
    public var harness: Harness?
}

/// The `{events, diagnostics, skipped}` envelope returned by `readUsage`.
/// `skipped` maps a harness id to a list of skip-reason strings.
public struct ReadUsageResult: Codable, Sendable, Equatable {
    public var events: [UsageEvent]
    public var diagnostics: [Diagnostic]
    public var skipped: [String: [String]]
}

/// The result of `detectHarnesses`: harnesses whose data is readable, and the
/// ones the tool knows about but cannot read over this ABI (always empty here).
public struct Detection: Codable, Sendable, Equatable {
    public var supported: [Harness]
    public var unsupported: [Harness]
}

/// One aggregated rollup bucket. `costUsd` is omitted when absent (never
/// serialized as null), matching the core.
public struct Rollup: Codable, Sendable, Equatable {
    public var key: String
    public var tokens: TokenCounts
    public var events: UInt64
    public var turns: UInt64
    public var calls: UInt64
    public var costUsd: Double?
}

/// One price tier of a tiered model price.
public struct PriceTier: Codable, Sendable, Equatable {
    public var threshold: Double
    public var input: Double
    public var output: Double
    public var cacheRead: Double?
    public var cacheWrite: Double?
    public var cacheWrite1h: Double?
}

/// A per-million-token price for a model (flat, or tiered via `tiers`). Rates
/// are USD per million tokens.
public struct ModelPrice: Codable, Sendable, Equatable {
    public var input: Double
    public var output: Double
    public var cacheRead: Double?
    public var cacheWrite: Double?
    public var cacheWrite1h: Double?
    public var tiers: [PriceTier]?
    public var tierMode: String?

    public init(
        input: Double,
        output: Double,
        cacheRead: Double? = nil,
        cacheWrite: Double? = nil,
        cacheWrite1h: Double? = nil,
        tiers: [PriceTier]? = nil,
        tierMode: String? = nil
    ) {
        self.input = input
        self.output = output
        self.cacheRead = cacheRead
        self.cacheWrite = cacheWrite
        self.cacheWrite1h = cacheWrite1h
        self.tiers = tiers
        self.tierMode = tierMode
    }
}

/// The provenance of one loaded pricing catalog. `fetchedAt` is null for the
/// override catalog (the wire always emits the key, so it is a non-omitted
/// optional here to match the capi's `catalog_info_json`).
public struct CatalogInfo: Codable, Sendable, Equatable {
    public var source: String
    public var fetchedAt: String?
    public var models: UInt64
}

/// The priced-group pricing payload attached to each `PricedRollup`: a hit
/// (`priced == true`) or a miss (`priced == false`). The `priced` discriminant
/// selects which fields are populated. Mirrors the flattened Go `PriceLookup`
/// shape (sdks/go/skopli/types.go) and the capi `wire.rs` hit/miss encoders.
public struct PriceLookup: Codable, Sendable, Equatable {
    public var priced: Bool

    // Common / hit fields.
    public var model: String?
    public var key: String?
    public var price: ModelPrice?
    public var source: String?
    public var fetchedAt: String?
    public var usd: Double?
    /// Set on a hit when a tiered model was aggregated across multiple calls.
    public var tieredAggregate: Bool?
    /// Set when a group priced multiple distinct models.
    public var models: [String]?

    // Miss fields.
    public var attempted: [String]?
    public var reason: String?
}

/// A rollup bucket with its pricing lookup attached.
public struct PricedRollup: Codable, Sendable, Equatable {
    public var key: String
    public var tokens: TokenCounts
    public var events: UInt64
    public var turns: UInt64
    public var calls: UInt64
    public var costUsd: Double?
    public var pricing: PriceLookup
}
