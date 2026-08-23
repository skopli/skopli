import Foundation
import XCTest

@testable import Skopli

// Billing-block windowing conformance, driven by the SHARED fixtures in
// golden/rollup-block/. Each case rolls one synthetic event per timestamp into
// blocks of the case's width in the case's zone and asserts the resulting blocks
// equal the shared gold, exercising the same native core the Rust and
// other-language suites use.

final class RollupBlockTests: XCTestCase {
    private func repoRoot() -> URL {
        URL(fileURLWithPath: #filePath)
            .deletingLastPathComponent()  // SkopliTests
            .deletingLastPathComponent()  // Tests
            .deletingLastPathComponent()  // swift
            .deletingLastPathComponent()  // sdks
            .deletingLastPathComponent()  // <root>
    }

    private struct Case: Decodable {
        struct Expected: Decodable {
            let key: String
            let events: UInt64
        }
        let name: String
        let tz: String
        let blockMs: Int64
        let timestamps: [String]
        let expected: [Expected]
    }

    func testRollupBlockConformance() throws {
        let url = repoRoot()
            .appendingPathComponent("golden")
            .appendingPathComponent("rollup-block")
            .appendingPathComponent("cases.json")
        let cases = try JSONDecoder().decode([Case].self, from: try Data(contentsOf: url))
        XCTAssertFalse(cases.isEmpty, "loaded block cases")

        for kase in cases {
            let events = kase.timestamps.map { ts in
                UsageEvent(
                    harness: .opencode, timestamp: ts, sessionId: "s", messageId: "m",
                    turn: true, model: "m", tokens: TokenCounts(input: 0, output: 0))
            }
            let got = try Skopli.rollup(events: events, by: .block, tz: kase.tz, blockMs: kase.blockMs)
            let actual = got.map { Case.Expected(key: $0.key, events: $0.events) }
            XCTAssertEqual(
                actual.map { [$0.key, String($0.events)] },
                kase.expected.map { [$0.key, String($0.events)] },
                "rollup-block: \(kase.name)")
        }
    }

    // Omitting blockMs (a nil the encoder drops) must apply the five-hour
    // default: two events 3h41m apart join one block anchored to 09:00, and an
    // event past five hours opens a new block. Guards the facade's
    // absent-optional serialization, not the fixture's explicit width.
    func testRollupBlockOmittedWidthUsesDefault() throws {
        func events(_ timestamps: String...) -> [UsageEvent] {
            timestamps.map { ts in
                UsageEvent(
                    harness: .opencode, timestamp: ts, sessionId: "s", messageId: "m",
                    turn: true, model: "m", tokens: TokenCounts(input: 0, output: 0))
            }
        }

        let joined = try Skopli.rollup(
            events: events("2026-01-01T09:17:00.000Z", "2026-01-01T13:00:00.000Z"),
            by: .block, tz: "UTC")
        XCTAssertEqual(
            joined.map { [$0.key, String($0.events)] },
            [["2026-01-01T09:00:00.000Z", "2"]],
            "omitted-width within five hours joins")

        let split = try Skopli.rollup(
            events: events("2026-01-01T09:00:00.000Z", "2026-01-01T14:30:00.000Z"),
            by: .block, tz: "UTC")
        XCTAssertEqual(
            split.map { [$0.key, String($0.events)] },
            [["2026-01-01T09:00:00.000Z", "1"], ["2026-01-01T14:00:00.000Z", "1"]],
            "omitted-width past five hours splits")
    }
}
