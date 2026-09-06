import Foundation
import XCTest

@testable import Skopli

// Timezone day-bucketing conformance, driven by the SHARED fixtures in
// golden/rollup-tz/. Each case rolls one synthetic event per timestamp up by
// day in the case's zone and asserts the resulting buckets equal the shared
// gold, exercising the same native core the Rust and other-language suites use.

final class RollupTzTests: XCTestCase {
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
        let timestamps: [String]
        let expected: [Expected]
    }

    func testRollupTzConformance() throws {
        let url = repoRoot()
            .appendingPathComponent("golden")
            .appendingPathComponent("rollup-tz")
            .appendingPathComponent("cases.json")
        let cases = try JSONDecoder().decode([Case].self, from: try Data(contentsOf: url))
        XCTAssertFalse(cases.isEmpty, "loaded tz cases")

        for kase in cases {
            let events = kase.timestamps.map { ts in
                UsageEvent(
                    harness: .opencode, timestamp: ts, sessionId: "s", messageId: "m",
                    turn: true, model: "m", tokens: TokenCounts(input: 0, output: 0))
            }
            let got = try Skopli.rollup(events: events, by: .day, tz: kase.tz)
            let actual = got.map { Case.Expected(key: $0.key, events: $0.events) }
            XCTAssertEqual(
                actual.map { [$0.key, String($0.events)] },
                kase.expected.map { [$0.key, String($0.events)] },
                "rollup-tz: \(kase.name)")
        }
    }
}
