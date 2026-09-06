import Foundation
import XCTest

@testable import Skopli

// Decode the pi-ai family harness ids that a rebuilt core now emits on the wire,
// plus an unknown id that must degrade to `.other` rather than throwing (the
// forward-compatible fallback that mirrors Java's Harness.fromId -> OTHER).

final class HarnessTests: XCTestCase {
    private func event(harnessId: String) -> Data {
        let json = """
            {
              "harness": "\(harnessId)",
              "timestamp": "2026-08-01T00:00:00.000Z",
              "sessionId": "s1",
              "messageId": "m1",
              "turn": true,
              "subagent": false,
              "model": "gpt-5",
              "tokens": {
                "input": 10,
                "output": 5,
                "cacheRead": 0,
                "cacheWrite": 0,
                "reasoning": 0
              }
            }
            """
        return Data(json.utf8)
    }

    func testDecodesNewHarnessIds() throws {
        let decoder = JSONDecoder()
        for id in ["prime", "gajae", "kimchi", "mimocode", "commandcode"] {
            let ev = try decoder.decode(UsageEvent.self, from: event(harnessId: id))
            XCTAssertEqual(ev.harness, Harness(rawValue: id))
            XCTAssertEqual(ev.harness.rawValue, id)
        }
    }

    func testUnknownHarnessIdDecodesToOther() throws {
        let ev = try JSONDecoder().decode(UsageEvent.self, from: event(harnessId: "totally-new-9000"))
        XCTAssertEqual(ev.harness, .other)
    }

    func testKnownHarnessEncodeRoundTrips() throws {
        for harness in [Harness.claude, .prime, .gajae, .kimchi, .mimocode, .commandcode] {
            let data = try JSONEncoder().encode(harness)
            let back = try JSONDecoder().decode(Harness.self, from: data)
            XCTAssertEqual(back, harness)
        }
    }
}
