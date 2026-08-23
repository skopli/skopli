import Foundation
import XCTest

@testable import Skopli

// Cache fetchedAt boundary conformance, driven by the SHARED fixtures in
// golden/pricing/timestamps/. The stamp is one wire contract read and written by
// seven facades; each case is read and written through this facade's own cache
// path (loadCached / storeCached) so Swift and the native core agree on every
// boundary. A valid stamp resolves to its epoch, bracketed by a 10 ms guard
// band (now == epochMs - 10 fresh, now == epochMs + 10 stale) with a zero TTL:
// gross misparses flip these assertions while the facade's sub-millisecond
// floating-point age arithmetic never does. An invalid stamp yields nil.

final class TimestampsTests: XCTestCase {
    private func repoRoot() -> URL {
        URL(fileURLWithPath: #filePath)
            .deletingLastPathComponent()  // SkopliTests
            .deletingLastPathComponent()  // Tests
            .deletingLastPathComponent()  // swift
            .deletingLastPathComponent()  // sdks
            .deletingLastPathComponent()  // <root>
    }

    private struct ReadCase: Decodable {
        let name: String
        let stamp: String
        let epochMs: Int64?
    }
    private struct Cases: Decodable {
        let read: [ReadCase]
        let write: [Int64]
    }

    private func date(_ ms: Int64) -> Date {
        Date(timeIntervalSince1970: Double(ms) / 1000)
    }

    private func writeStamp(_ dir: String, _ name: String, _ stamp: String) throws {
        let encoded = String(data: try JSONEncoder().encode(stamp), encoding: .utf8)!
        let body = "{\"fetchedAt\":\(encoded),\"payload\":{}}"
        try Data(body.utf8).write(
            to: URL(fileURLWithPath: dir).appendingPathComponent(name))
    }

    private func cases() throws -> Cases {
        let url = repoRoot()
            .appendingPathComponent("golden")
            .appendingPathComponent("pricing")
            .appendingPathComponent("timestamps")
            .appendingPathComponent("cases.json")
        return try JSONDecoder().decode(Cases.self, from: try Data(contentsOf: url))
    }

    func testTimestampReadConformance() throws {
        let dir = NSTemporaryDirectory() + "skopli-ts-read-" + UUID().uuidString
        try FileManager.default.createDirectory(
            atPath: dir, withIntermediateDirectories: true)
        let name = "pricing-openrouter.json"

        for kase in try cases().read {
            try writeStamp(dir, name, kase.stamp)
            guard let epochMs = kase.epochMs else {
                XCTAssertNil(loadCached(dir, name, 0, date(0)), "timestamps: \(kase.name)")
                continue
            }
            let fresh = loadCached(dir, name, 0, date(epochMs - 10))
            let stale = loadCached(dir, name, 0, date(epochMs + 10))
            XCTAssertNotNil(fresh, "timestamps: \(kase.name)")
            XCTAssertEqual(fresh?.stale, false, "timestamps: \(kase.name)")
            XCTAssertNotNil(stale, "timestamps: \(kase.name)")
            XCTAssertEqual(stale?.stale, true, "timestamps: \(kase.name)")
        }
    }

    func testTimestampWriteConformance() throws {
        let dir = NSTemporaryDirectory() + "skopli-ts-write-" + UUID().uuidString
        try FileManager.default.createDirectory(
            atPath: dir, withIntermediateDirectories: true)
        let name = "pricing-openrouter.json"

        for ms in try cases().write {
            let stamp = try storeCached(dir, name, Data("{}".utf8), date(ms))
            XCTAssertEqual(stamp.count, 24, "timestamps: write \(ms)")
            XCTAssertTrue(stamp.hasSuffix("Z"), "timestamps: write \(ms)")
            let fresh = loadCached(dir, name, 0, date(ms - 10))
            let stale = loadCached(dir, name, 0, date(ms + 10))
            XCTAssertEqual(fresh?.stale, false, "timestamps: write \(ms)")
            XCTAssertEqual(stale?.stale, true, "timestamps: write \(ms)")
        }
    }
}
