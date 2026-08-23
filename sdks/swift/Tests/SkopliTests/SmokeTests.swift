import Foundation
import XCTest

@testable import Skopli

// Smoke test driving the Swift facade over the C ABI end to end: read -> rollup
// -> price against the committed golden case, asserting structural parity with
// the shared conformance gold (the same golden/claude/basic +
// golden/pricing/catalogs data the Rust capi and the Go/Java/C# smokes use).
//
// This test runs in CI on a macOS runner. It needs the capi cdylib on the
// loader path at run time (or the staticlib linked in); see README.md.

final class SmokeTests: XCTestCase {
    // MARK: - Repo layout

    /// Walk up from this test file to the workspace root:
    ///   SmokeTests.swift -> SkopliTests -> Tests -> swift -> sdks -> <root>
    private func repoRoot() -> URL {
        URL(fileURLWithPath: #filePath)
            .deletingLastPathComponent()  // SkopliTests
            .deletingLastPathComponent()  // Tests
            .deletingLastPathComponent()  // swift
            .deletingLastPathComponent()  // sdks
            .deletingLastPathComponent()  // <root>
    }

    private func claudeInputDir() -> String {
        repoRoot()
            .appendingPathComponent("golden")
            .appendingPathComponent("claude")
            .appendingPathComponent("basic")
            .appendingPathComponent("input")
            .path
    }

    private func claudeOptions() -> ReadUsageOptions {
        ReadUsageOptions(
            home: "/nonexistent",
            env: ["CLAUDE_CONFIG_DIR": claudeInputDir()],
            harnesses: [.claude],
            tz: "UTC"
        )
    }

    // MARK: - Meta

    func testMeta() throws {
        XCTAssertEqual(Skopli.abiVersion, 1)
        XCTAssertEqual(Skopli.schemaVersion, 1)
        XCTAssertFalse(Skopli.version.isEmpty, "version string non-empty")
        let dir = try Skopli.defaultCacheDir()
        XCTAssertTrue(dir.contains("skopli"), "cache dir mentions skopli: \(dir)")
    }

    // MARK: - harness enum parity

    /// The public `Harness` enum must have a case for every id in the shared
    /// registry-id fixture (`golden/registry/ids.json`): each id must construct a
    /// concrete case (never `.other`), so a reader added to the core cannot
    /// silently collapse to the fallback in the Swift surface.
    func testHarnessEnumCoversRegistry() throws {
        struct RegistryIds: Decodable { let ids: [String] }
        let url = repoRoot()
            .appendingPathComponent("golden")
            .appendingPathComponent("registry")
            .appendingPathComponent("ids.json")
        let fixture = try JSONDecoder().decode(RegistryIds.self, from: Data(contentsOf: url))
        for id in fixture.ids {
            let harness = Harness(rawValue: id)
            XCTAssertNotNil(harness, "Harness enum missing registry id \(id)")
            XCTAssertNotEqual(harness, .other, "Harness enum missing registry id \(id)")
        }
        let registered = Set(fixture.ids)
        for harness in Harness.allCases where harness != .other {
            XCTAssertTrue(
                registered.contains(harness.rawValue),
                "Harness enum has id \(harness.rawValue) the registry does not"
            )
        }
    }

    // MARK: - cost_usd

    func testCostUSDFlat() throws {
        let tokens = TokenCounts(input: 1000, output: 500)
        let price = ModelPrice(input: 1.25, output: 10.0)
        let got = try Skopli.costUSD(tokens: tokens, price: price)
        let want = (1000.0 * 1.25 + 500.0 * 10.0) / 1_000_000.0
        XCTAssertEqual(got, want, accuracy: 1e-12)
    }

    // MARK: - detect

    func testDetectFindsClaude() throws {
        let opts = Options(home: "/nonexistent", env: ["CLAUDE_CONFIG_DIR": claudeInputDir()])
        let detection = try Skopli.detectHarnesses(options: opts)
        XCTAssertTrue(detection.supported.contains(.claude), "claude detected: \(detection)")
        XCTAssertTrue(detection.unsupported.isEmpty, "unsupported empty over this ABI")
    }

    // MARK: - read -> rollup round trip vs gold

    func testReadRollupRoundtripMatchesGold() throws {
        let result = try Skopli.readUsage(options: claudeOptions())
        XCTAssertFalse(result.events.isEmpty, "read produced events")

        // Sort events the way the exporter/gold does before rolling up.
        let events = result.events.sorted { a, b in
            let ka = [a.timestamp, a.sessionId, a.messageId, a.model]
            let kb = [b.timestamp, b.sessionId, b.messageId, b.model]
            for (x, y) in zip(ka, kb) where x != y { return x < y }
            return false
        }

        let byModel = try Skopli.rollup(events: events, by: .model, tz: "UTC")

        // Compare structurally to the gold's model lane (round-trip both sides
        // through JSON so optional/absent encoding differences vanish).
        let goldURL = repoRoot()
            .appendingPathComponent("golden")
            .appendingPathComponent("claude")
            .appendingPathComponent("basic")
            .appendingPathComponent("expected-rollup.json")
        let goldData = try Data(contentsOf: goldURL)
        let gold = try JSONDecoder().decode(GoldRollup.self, from: goldData)

        XCTAssertEqual(
            normalized(byModel), normalized(gold.by.model),
            "rollup(by=model) must equal the core gold model lane")
    }

    // MARK: - pricing round trip

    private func readCatalogPayload(_ name: String) throws -> RawJSON {
        let url = repoRoot()
            .appendingPathComponent("golden")
            .appendingPathComponent("pricing")
            .appendingPathComponent("catalogs")
            .appendingPathComponent("\(name).json")
        return RawJSON(try Data(contentsOf: url))
    }

    /// The exact synthetic events from the pricing conformance case (mirrors the
    /// capi/Go smoke tests).
    private func pricingEvents() -> [UsageEvent] {
        func ev(_ mid: String, _ ts: String, _ model: String, _ tk: TokenCounts) -> UsageEvent {
            UsageEvent(
                harness: .opencode, timestamp: ts, sessionId: "s", messageId: mid,
                turn: true, model: model, tokens: tk)
        }
        return [
            ev("flat", "2026-08-01T00:00:00.000Z", "gpt-5", TokenCounts(input: 1000, output: 500)),
            ev(
                "tiered", "2026-08-01T00:01:00.000Z", "claude-sonnet-4-5",
                TokenCounts(
                    input: 200_000, output: 10_000, cacheRead: 20_000, cacheWrite: 30_000,
                    cacheWrite1h: 10_000, reasoning: 2_000)),
            ev(
                "base-1h", "2026-08-01T00:02:00.000Z", "claude-sonnet-4-5",
                TokenCounts(
                    input: 5_000, output: 1_000, cacheRead: 2_000, cacheWrite: 4_000,
                    cacheWrite1h: 1_500)),
            ev(
                "alias", "2026-08-01T00:03:00.000Z", "us.anthropic.claude-opus-4-6-20260115-v1:0",
                TokenCounts(input: 800, output: 200)),
            ev(
                "miss", "2026-08-01T00:04:00.000Z", "totally-unknown-model-9000",
                TokenCounts(input: 100, output: 100)),
        ]
    }

    func testPricingRoundtrip() throws {
        let pinned = "2026-08-01T00:00:00.000Z"
        let pricing = try Skopli.createPricing(
            options: PricingOptions(
                mode: .calculate,
                catalogs: [
                    Catalog(
                        source: "openrouter", fetchedAt: pinned, format: "openrouter",
                        payload: try readCatalogPayload("openrouter")),
                    Catalog(
                        source: "litellm", fetchedAt: pinned, format: "litellm",
                        payload: try readCatalogPayload("litellm")),
                ],
                sources: []))
        defer { pricing.close() }

        let infos = try pricing.catalogs()
        XCTAssertEqual(infos.count, 2)
        XCTAssertEqual(infos[0].source, "openrouter")
        XCTAssertEqual(infos[1].source, "litellm")
        XCTAssertGreaterThan(infos[0].models, 0)

        let priced = try pricing.priceEvents(events: pricingEvents(), by: .model)
        XCTAssertFalse(priced.isEmpty)

        guard let gpt5 = priced.first(where: { $0.key == "gpt-5" }) else {
            return XCTFail("gpt-5 group present")
        }
        XCTAssertTrue(gpt5.pricing.priced)
        // Flat gpt-5: (1000*1.25 + 500*10)/1e6 = 0.00625 (agrees with the gold).
        XCTAssertEqual(gpt5.pricing.usd ?? -1, 0.00625, accuracy: 1e-9)

        guard let miss = priced.first(where: { $0.key == "totally-unknown-model-9000" }) else {
            return XCTFail("miss group present")
        }
        XCTAssertFalse(miss.pricing.priced)
        XCTAssertEqual(miss.pricing.attempted, ["totally-unknown-model-9000"])
    }

    // MARK: - errors thrown

    func testInvalidArgumentThrows() throws {
        XCTAssertThrowsError(try Skopli.readUsage(options: ReadUsageOptions(since: "not-a-date")))
        { error in
            guard case SkopliError.invalidArgument = error else {
                return XCTFail("expected .invalidArgument, got \(error)")
            }
        }
    }

    func testCatalogErrorThrows() throws {
        // A catalog with an unknown format triggers AG_STATUS_CATALOG.
        XCTAssertThrowsError(
            try Skopli.createPricing(
                options: PricingOptions(
                    catalogs: [
                        Catalog(
                            source: "bogus", format: "not-a-real-format",
                            payload: RawJSON(Data("{}".utf8)))
                    ],
                    sources: []))
        ) { error in
            guard case SkopliError.catalog = error else {
                return XCTFail("expected .catalog, got \(error)")
            }
        }
    }

    // MARK: - deinit frees (no crash / no leak assertion at the ABI level)

    func testPricingDeinitFrees() throws {
        // Construct and drop without close(); deinit must free the handle. A
        // double free would trip the ABI's guard; reaching here cleanly (plus the
        // explicit-close idempotency below) exercises the deinit path.
        do {
            let p = try Skopli.createPricing(options: PricingOptions(sources: []))
            _ = try p.catalogs()
        }  // p deinits here, freeing the handle.

        // Explicit close is idempotent.
        let p2 = try Skopli.createPricing(options: PricingOptions(sources: []))
        p2.close()
        p2.close()
        // Use-after-close throws rather than crashing.
        XCTAssertThrowsError(try p2.catalogs())
    }

    // MARK: - helpers

    /// Round-trip a value through JSON into a normalized untyped form so optional
    /// vs value and omit-when-nil encoding differences do not defeat equality.
    private func normalized<T: Encodable>(_ value: T) -> JSONAny {
        let data = try! JSONEncoder().encode(value)
        return try! JSONDecoder().decode(JSONAny.self, from: data)
    }
}

/// The shape of golden/claude/basic/expected-rollup.json needed here.
private struct GoldRollup: Decodable {
    struct By: Decodable { let model: [Rollup] }
    let by: By
}
