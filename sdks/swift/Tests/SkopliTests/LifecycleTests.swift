import Foundation
import XCTest

@testable import Skopli

// Pricing lifecycle + new-export conformance, driven by the SHARED fixtures in
// golden/pricing/lifecycle/ (the five-behavior matrix) and golden/pricing/basic/
// (rollup gold equality, lookup hit + miss). A facade that drifts from the TS
// reference fails against the shared gold rather than its author's assumptions.

final class LifecycleTests: XCTestCase {
    // MARK: - Repo layout

    private func repoRoot() -> URL {
        URL(fileURLWithPath: #filePath)
            .deletingLastPathComponent()  // SkopliTests
            .deletingLastPathComponent()  // Tests
            .deletingLastPathComponent()  // swift
            .deletingLastPathComponent()  // sdks
            .deletingLastPathComponent()  // <root>
    }

    private func lifecycleDir() -> URL {
        repoRoot()
            .appendingPathComponent("golden")
            .appendingPathComponent("pricing")
            .appendingPathComponent("lifecycle")
    }

    private func loadFixture<T: Decodable>(_ type: T.Type, _ segments: String...) throws -> T {
        var url = lifecycleDir()
        for segment in segments { url = url.appendingPathComponent(segment) }
        return try JSONDecoder().decode(T.self, from: Data(contentsOf: url))
    }

    // MARK: - Fixture shapes

    private struct LifecycleRequest: Decodable {
        let source: String
        let cacheFileName: String
        let ttlMs: Int64
        let now: String
        let nowSecond: String
        let fetchedAtFresh: String
        let fetchedAtStale: String
        let fetchedAtFetched: String
        let options: OptionsBlock
        let rollup: Rollup

        struct OptionsBlock: Decodable { let mode: PricingMode }
    }

    private struct LifecycleExpected: Decodable {
        let behavior: String
        let fetched: Bool
        let fetchedAt: String?
        let priced: Bool
        let pricedUsd: Double
    }

    private struct CacheFileShape: Decodable {
        let fetchedAt: String
        let payload: RawJSON
    }

    private func expected(_ behavior: String) throws -> LifecycleExpected {
        let exp = try loadFixture(LifecycleExpected.self, "expected", "\(behavior).json")
        XCTAssertEqual(exp.behavior, behavior, "behavior mismatch")
        return exp
    }

    // MARK: - Recording fetch

    /// Counts calls, then serves the payload (or throws when `throws` is set).
    private final class RecordingFetch: @unchecked Sendable {
        let payload: Data
        let throws_: Bool
        private let lock = NSLock()
        private(set) var calls = 0

        init(payload: Data, throws_: Bool = false) {
            self.payload = payload
            self.throws_ = throws_
        }

        func fetch(_ url: URL) throws -> Data {
            lock.lock()
            calls += 1
            lock.unlock()
            if throws_ { throw SkopliError.internalError("network down") }
            return payload
        }
    }

    private func parseISOTest(_ iso: String) -> Date {
        let formatter = DateFormatter()
        formatter.locale = Locale(identifier: "en_US_POSIX")
        formatter.timeZone = TimeZone(identifier: "UTC")
        formatter.dateFormat = "yyyy-MM-dd'T'HH:mm:ss.SSSZZZZZ"
        return formatter.date(from: iso)!
    }

    private func fixedClock(_ iso: String) -> @Sendable () -> Date {
        let date = parseISOTest(iso)
        return { date }
    }

    /// A clock whose value the test can advance to exercise TTL-expiry reloads.
    private final class MutableClock: @unchecked Sendable {
        private let lock = NSLock()
        private var date: Date
        init(_ date: Date) { self.date = date }
        func set(_ date: Date) {
            lock.lock(); defer { lock.unlock() }
            self.date = date
        }
        func now() -> Date {
            lock.lock(); defer { lock.unlock() }
            return date
        }
    }

    /// The canonical raw source payload named by the fixture README (the bytes a
    /// live fetch would return), read verbatim from source-<name>.json.
    private func sourcePayload(_ fixture: String) throws -> Data {
        try Data(contentsOf: lifecycleDir().appendingPathComponent(fixture))
    }

    /// Select the source factory named by `request.source`.
    private func sourceFactory(_ source: String, _ url: String) -> PricingSource {
        switch source {
        case "openrouter": return openRouterSource(url: url)
        case "litellm": return liteLlmSource(url: url)
        case "models-dev": return modelsDevSource(url: url)
        default: fatalError("unknown request source: \(source)")
        }
    }

    private func installCache(_ dir: URL, _ name: String, _ fixture: String) throws {
        let raw = try Data(contentsOf: lifecycleDir().appendingPathComponent(fixture))
        try raw.write(to: dir.appendingPathComponent(name))
    }

    private func currentFetchedAt(_ dir: URL, _ name: String) -> String? {
        let url = dir.appendingPathComponent(name)
        guard let raw = try? Data(contentsOf: url),
            let file = try? JSONDecoder().decode(CacheFileShape.self, from: raw)
        else { return nil }
        return file.fetchedAt
    }

    private func tempDir() throws -> URL {
        let dir = FileManager.default.temporaryDirectory
            .appendingPathComponent("skopli-lifecycle-\(UUID().uuidString)")
        try FileManager.default.createDirectory(at: dir, withIntermediateDirectories: true)
        addTeardownBlock { try? FileManager.default.removeItem(at: dir) }
        return dir
    }

    private let sourceURL = "https://or.test/models"

    /// Drive one behavior: build the source named by `req.source` over the
    /// injected fetch/clock and a temp cacheDir, price the shared rollup, return
    /// it.
    private func runLifecycle(
        _ req: LifecycleRequest, _ dir: URL, _ stub: RecordingFetch, offline: Bool, refresh: Bool
    ) throws -> PricedRollup {
        let pricing = try Skopli.createPricing(
            options: PricingOptions(
                mode: req.options.mode,
                sources: [sourceFactory(req.source, sourceURL)],
                cacheDir: dir.path,
                offline: offline,
                ttlMs: req.ttlMs,
                refresh: refresh,
                fetch: { url in try stub.fetch(url) },
                now: fixedClock(req.now)))
        defer { pricing.close() }
        let priced = try pricing.priceRollups(rollups: [req.rollup])
        XCTAssertEqual(priced.count, 1, "want 1 priced rollup")
        return priced[0]
    }

    private func assertLifecycle(
        _ exp: LifecycleExpected, _ stub: RecordingFetch, _ got: PricedRollup, _ dir: URL,
        _ name: String
    ) {
        XCTAssertEqual(stub.calls, exp.fetched ? 1 : 0, "\(exp.behavior): fetch call count")
        XCTAssertEqual(got.pricing.priced, exp.priced, "\(exp.behavior): priced")
        XCTAssertEqual(got.pricing.usd ?? 0, exp.pricedUsd, accuracy: 1e-9, "\(exp.behavior): usd")
        let stamp = currentFetchedAt(dir, name)
        if let want = exp.fetchedAt {
            XCTAssertEqual(stamp, want, "\(exp.behavior): cache fetchedAt")
        } else {
            XCTAssertNil(stamp, "\(exp.behavior): expected no cache file")
        }
    }

    // MARK: - Five-behavior matrix

    func testColdFetch() throws {
        let req = try loadFixture(LifecycleRequest.self, "request.json")
        let exp = try expected("cold-fetch")
        let dir = try tempDir()
        let stub = RecordingFetch(payload: try sourcePayload("source-openrouter.json"))
        let got = try runLifecycle(req, dir, stub, offline: false, refresh: false)
        assertLifecycle(exp, stub, got, dir, req.cacheFileName)
        XCTAssertEqual(exp.fetchedAt, req.fetchedAtFetched)
    }

    func testWarmCache() throws {
        let req = try loadFixture(LifecycleRequest.self, "request.json")
        let exp = try expected("warm-cache")
        let dir = try tempDir()
        try installCache(dir, req.cacheFileName, "cache-fresh.json")
        let stub = RecordingFetch(payload: try sourcePayload("source-openrouter.json"))
        let got = try runLifecycle(req, dir, stub, offline: false, refresh: false)
        assertLifecycle(exp, stub, got, dir, req.cacheFileName)
        XCTAssertEqual(exp.fetchedAt, req.fetchedAtFresh)
    }

    func testTTLRefresh() throws {
        let req = try loadFixture(LifecycleRequest.self, "request.json")
        let exp = try expected("ttl-refresh")
        let dir = try tempDir()
        try installCache(dir, req.cacheFileName, "cache-stale.json")
        let stub = RecordingFetch(payload: try sourcePayload("source-openrouter.json"))
        let got = try runLifecycle(req, dir, stub, offline: false, refresh: true)
        assertLifecycle(exp, stub, got, dir, req.cacheFileName)
        XCTAssertEqual(exp.fetchedAt, req.fetchedAtFetched)
    }

    func testOffline() throws {
        let req = try loadFixture(LifecycleRequest.self, "request.json")
        let exp = try expected("offline")
        let dir = try tempDir()
        try installCache(dir, req.cacheFileName, "cache-stale.json")
        let stub = RecordingFetch(payload: try sourcePayload("source-openrouter.json"))
        let got = try runLifecycle(req, dir, stub, offline: true, refresh: false)
        assertLifecycle(exp, stub, got, dir, req.cacheFileName)
        XCTAssertEqual(exp.fetchedAt, req.fetchedAtStale)
    }

    func testOfflineNoCache() throws {
        let req = try loadFixture(LifecycleRequest.self, "request.json")
        let exp = try expected("offline-no-cache")
        let dir = try tempDir()
        let stub = RecordingFetch(payload: try sourcePayload("source-openrouter.json"))
        let got = try runLifecycle(req, dir, stub, offline: true, refresh: false)
        assertLifecycle(exp, stub, got, dir, req.cacheFileName)
        XCTAssertFalse(got.pricing.priced, "offline-no-cache should degrade to a miss")
    }

    func testStaleFallback() throws {
        let req = try loadFixture(LifecycleRequest.self, "request.json")
        let exp = try expected("stale-fallback")
        let dir = try tempDir()
        try installCache(dir, req.cacheFileName, "cache-stale.json")
        let stub = RecordingFetch(payload: try sourcePayload("source-openrouter.json"), throws_: true)
        let got = try runLifecycle(req, dir, stub, offline: false, refresh: false)
        assertLifecycle(exp, stub, got, dir, req.cacheFileName)
        XCTAssertEqual(exp.fetchedAt, req.fetchedAtStale)
    }

    // MARK: - The three hardening behaviors

    func testRequestFieldsConsumed() throws {
        // Guard that every request-level field the harness relies on is present
        // and matches the pinned shared contract (a fixture rename fails loudly).
        let req = try loadFixture(LifecycleRequest.self, "request.json")
        XCTAssertEqual(req.source, "openrouter", "request.source")
        XCTAssertEqual(req.cacheFileName, "pricing-openrouter.json", "request.cacheFileName")
        XCTAssertEqual(req.options.mode, .calculate, "request.options.mode")
    }

    func testFetchedEmpty() throws {
        // Fetch succeeds but returns a zero-price payload: treated like a fetch
        // failure - the stale catalog is served and the cache is NOT overwritten.
        let req = try loadFixture(LifecycleRequest.self, "request.json")
        let exp = try expected("fetched-empty")
        let dir = try tempDir()
        try installCache(dir, req.cacheFileName, "cache-stale.json")
        let stub = RecordingFetch(payload: try sourcePayload("source-openrouter-empty.json"))
        let got = try runLifecycle(req, dir, stub, offline: false, refresh: false)
        assertLifecycle(exp, stub, got, dir, req.cacheFileName)
        XCTAssertEqual(exp.fetchedAt, req.fetchedAtStale, "stale stamp unchanged")
    }

    func testCachedEmpty() throws {
        // A fresh cache that parses to zero prices is unusable, same as no cache:
        // a fetch is issued and the cache is rewritten with fetchedAt = now.
        let req = try loadFixture(LifecycleRequest.self, "request.json")
        let exp = try expected("cached-empty")
        let dir = try tempDir()
        try installCache(dir, req.cacheFileName, "cache-fresh-empty.json")
        let stub = RecordingFetch(payload: try sourcePayload("source-openrouter.json"))
        let got = try runLifecycle(req, dir, stub, offline: false, refresh: false)
        assertLifecycle(exp, stub, got, dir, req.cacheFileName)
        XCTAssertEqual(exp.fetchedAt, req.fetchedAtFetched, "rewritten to now")
    }

    // MARK: - Table-driven per-source pass

    func testPerSourceMatrix() throws {
        let req = try loadFixture(LifecycleRequest.self, "request.json")
        struct Case { let source: String; let fixture: String; let cacheFile: String }
        let cases = [
            Case(source: "openrouter", fixture: "source-openrouter.json", cacheFile: "pricing-openrouter.json"),
            Case(source: "litellm", fixture: "source-litellm.json", cacheFile: "pricing-litellm.json"),
            Case(source: "models-dev", fixture: "source-modelsdev.json", cacheFile: "pricing-models-dev.json"),
        ]
        for c in cases {
            let dir = try tempDir()
            let stub = RecordingFetch(payload: try sourcePayload(c.fixture))
            let pricing = try Skopli.createPricing(
                options: PricingOptions(
                    mode: req.options.mode,
                    sources: [sourceFactory(c.source, "https://\(c.source).test/api")],
                    cacheDir: dir.path,
                    ttlMs: req.ttlMs,
                    fetch: { url in try stub.fetch(url) },
                    now: fixedClock(req.now)))
            defer { pricing.close() }

            let infos = try pricing.catalogs()
            XCTAssertEqual(infos.count, 1, "\(c.source): one catalog")
            XCTAssertEqual(infos[0].source, c.source, "\(c.source): provenance")
            XCTAssertGreaterThan(infos[0].models, 0, "\(c.source): has models")

            let priced = try pricing.priceRollups(rollups: [req.rollup])
            XCTAssertEqual(priced[0].pricing.usd ?? 0, 2.25, accuracy: 1e-9, "\(c.source): priced 2.25")
            XCTAssertEqual(stub.calls, 1, "\(c.source): fetched once")
            XCTAssertNotNil(
                currentFetchedAt(dir, c.cacheFile), "\(c.source): wrote \(c.cacheFile)")
        }
    }

    // MARK: - Live TTL reload

    private struct LiveReloadExpected: Decodable {
        let behavior: String
        let fetchesTotal: Int
        let fetchedAtFirst: String
        let fetchedAtSecond: String
        let priced: Bool
        let pricedUsd: Double
    }

    func testLiveReload() throws {
        let req = try loadFixture(LifecycleRequest.self, "request.json")
        let exp = try loadFixture(LiveReloadExpected.self, "expected", "live-reload.json")
        XCTAssertEqual(exp.behavior, "live-reload")
        let dir = try tempDir()
        let stub = RecordingFetch(payload: try sourcePayload("source-openrouter.json"))
        let clock = MutableClock(parseISOTest(req.now))

        let pricing = try Skopli.createPricing(
            options: PricingOptions(
                mode: req.options.mode,
                sources: [openRouterSource(url: sourceURL)],
                cacheDir: dir.path,
                ttlMs: req.ttlMs,
                fetch: { url in try stub.fetch(url) },
                now: { clock.now() }))
        defer { pricing.close() }

        // Query 1 at `now`: fetches and prices.
        let first = try pricing.priceRollups(rollups: [req.rollup])
        XCTAssertEqual(first[0].pricing.priced, exp.priced, "query 1 priced")
        XCTAssertEqual(first[0].pricing.usd ?? 0, exp.pricedUsd, accuracy: 1e-9, "query 1 usd")
        XCTAssertEqual(currentFetchedAt(dir, req.cacheFileName), exp.fetchedAtFirst, "first fetchedAt")

        // Advance the clock past the TTL; the SAME instance must reload.
        clock.set(parseISOTest(req.nowSecond))
        let second = try pricing.priceRollups(rollups: [req.rollup])
        XCTAssertEqual(second[0].pricing.priced, exp.priced, "query 2 priced")
        XCTAssertEqual(second[0].pricing.usd ?? 0, exp.pricedUsd, accuracy: 1e-9, "query 2 usd")
        XCTAssertEqual(currentFetchedAt(dir, req.cacheFileName), exp.fetchedAtSecond, "second fetchedAt")
        XCTAssertEqual(stub.calls, exp.fetchesTotal, "total fetches")
    }

    // MARK: - Live TTL reload failure retry

    /// A source whose Nth `load` throws (1-based), driving a reload failure. It
    /// serves the shared openrouter payload on every other call, so a successful
    /// load produces the priced-2.25 catalog.
    private final class FlakySource: PricingSource, @unchecked Sendable {
        let name = "openrouter"
        let format = "openrouter"
        let payload: Data
        let throwOnCall: Int
        private let lock = NSLock()
        private(set) var calls = 0

        init(payload: Data, throwOnCall: Int) {
            self.payload = payload
            self.throwOnCall = throwOnCall
        }

        func load(context: SourceContext) throws -> Catalog? {
            lock.lock()
            calls += 1
            let call = calls
            lock.unlock()
            if call == throwOnCall {
                throw SkopliError.internalError("reload fetch failed")
            }
            return Catalog(source: name, fetchedAt: isoUTC(context.now()), format: format, payload: RawJSON(payload))
        }
    }

    func testLiveReloadFailureRetries() throws {
        // Chosen TS-matching semantics: a rejected reload rejects THAT query
        // (the query that triggered the reload throws) and is not memoized, so
        // the NEXT query retries the reload. A prior handle whose completion
        // stamp is expired is not resurrected as fresh; it stays expired so the
        // retry reloads rather than silently serving it.
        let req = try loadFixture(LifecycleRequest.self, "request.json")
        let dir = try tempDir()
        let source = FlakySource(payload: try sourcePayload("source-openrouter.json"), throwOnCall: 2)
        let clock = MutableClock(parseISOTest(req.now))

        let pricing = try Skopli.createPricing(
            options: PricingOptions(
                mode: req.options.mode,
                sources: [source],
                cacheDir: dir.path,
                ttlMs: req.ttlMs,
                fetch: { _ in throw SkopliError.internalError("unused") },
                now: { clock.now() }))
        defer { pricing.close() }

        // Query 1 at `now`: the first load (call 1) succeeds and prices.
        let first = try pricing.priceRollups(rollups: [req.rollup])
        XCTAssertEqual(first[0].pricing.usd ?? 0, 2.25, accuracy: 1e-9, "query 1 usd")
        XCTAssertEqual(source.calls, 1, "query 1 loaded once")

        // Advance past the TTL. Query 2 triggers a reload whose load (call 2)
        // throws; the query itself errors and nothing is memoized.
        clock.set(parseISOTest(req.nowSecond))
        XCTAssertThrowsError(try pricing.priceRollups(rollups: [req.rollup]), "query 2 propagates the reload failure") { error in
            XCTAssertTrue(error is SkopliError, "reload failure surfaces as SkopliError")
        }
        XCTAssertEqual(source.calls, 2, "query 2 attempted the reload")

        // Query 3 (still past the TTL) retries the reload; the load (call 3)
        // succeeds. The retry proves the failed reload was not memoized.
        let third = try pricing.priceRollups(rollups: [req.rollup])
        XCTAssertEqual(third[0].pricing.usd ?? 0, 2.25, accuracy: 1e-9, "query 3 usd after retry")
        XCTAssertEqual(source.calls, 3, "query 3 retried the reload")
    }

    // MARK: - rollup gold equality + lookup hit/miss

    private func readCatalogPayload(_ name: String) throws -> RawJSON {
        let url = repoRoot()
            .appendingPathComponent("golden")
            .appendingPathComponent("pricing")
            .appendingPathComponent("catalogs")
            .appendingPathComponent("\(name).json")
        return RawJSON(try Data(contentsOf: url))
    }

    private func hermeticPricing() throws -> Pricing {
        let pinned = "2026-08-01T00:00:00.000Z"
        return try Skopli.createPricing(
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
    }

    func testPriceRollupsMatchesGold() throws {
        let pricing = try hermeticPricing()
        defer { pricing.close() }

        let goldURL = repoRoot()
            .appendingPathComponent("golden")
            .appendingPathComponent("pricing")
            .appendingPathComponent("basic")
            .appendingPathComponent("expected-priced-rollup.json")
        let goldData = try Data(contentsOf: goldURL)

        // The UNTOUCHED gold rollups as a raw JSON tree - never decoded through
        // facade types (a facade type that drops a gold field would otherwise
        // silently pass). This is the expected side of the equality.
        guard
            let root = try JSONSerialization.jsonObject(with: goldData) as? [String: Any],
            let rawRollups = root["rollups"] as? [[String: Any]]
        else {
            return XCTFail("gold rollups not an array of objects")
        }
        let expectedTree = try JSONDecoder().decode(
            JSONAny.self, from: JSONSerialization.data(withJSONObject: rawRollups))

        // Input is a raw copy of the gold rollups with ONLY the `pricing` field
        // removed from each element, decoded through the facade Rollup input type.
        let inputRollups = rawRollups.map { obj -> [String: Any] in
            var copy = obj
            copy.removeValue(forKey: "pricing")
            return copy
        }
        let inputData = try JSONSerialization.data(withJSONObject: inputRollups)
        let input = try JSONDecoder().decode([Rollup].self, from: inputData)

        let priced = try pricing.priceRollups(rollups: input)
        // Encode the actual returned values back to a raw JSON tree and compare
        // the COMPLETE tree against the untouched gold.
        XCTAssertEqual(
            normalized(priced), expectedTree,
            "priceRollups must equal the full raw gold tree")
    }

    func testLookupModelHitAndMiss() throws {
        let pricing = try hermeticPricing()
        defer { pricing.close() }

        let hit = try pricing.lookupModel("gpt-5")
        XCTAssertTrue(hit.priced, "gpt-5 should be a hit")
        XCTAssertEqual(hit.source, "litellm", "gpt-5 source")

        let miss = try pricing.lookupModel("totally-unknown-model-9000")
        XCTAssertFalse(miss.priced, "unknown model should be a miss")
    }

    // MARK: - shared golden constants contract

    private struct ConstantsSource: Decodable {
        let name: String
        let format: String
        let url: String
        let cacheFileName: String
    }

    private struct LifecycleConstants: Decodable {
        let sources: [ConstantsSource]
        let priorityOrder: [String]
        let defaultTtlMs: Int64
        let fetchTimeoutMs: Int64
        let cacheFileNamePattern: String
    }

    /// The shared golden constants contract (constants.json): the facade's own
    /// source names, formats, URLs, priority order, TTL default, fetch timeout,
    /// and cache filenames must equal the golden values so no facade's literals
    /// drift.
    func testLifecycleConstantsMatchGolden() throws {
        let constants = try loadFixture(LifecycleConstants.self, "constants.json")

        let urlByName = [
            "openrouter": BuiltinSourceURL.openRouter,
            "litellm": BuiltinSourceURL.liteLlm,
            "models-dev": BuiltinSourceURL.modelsDev,
        ]

        // the default built-in sources in the reference priority order
        let builtin = builtinSources().compactMap { $0 as? CachedSource }
        XCTAssertEqual(builtin.count, constants.priorityOrder.count, "builtin source count")
        XCTAssertEqual(builtin.map(\.name), constants.priorityOrder, "priority order")
        XCTAssertEqual(constants.sources.map(\.name), constants.priorityOrder, "sources order")

        let byName = Dictionary(uniqueKeysWithValues: builtin.map { ($0.name, $0) })
        for spec in constants.sources {
            guard let source = byName[spec.name] else {
                return XCTFail("no builtin source named \(spec.name)")
            }
            XCTAssertEqual(source.format, spec.format, "\(spec.name): format")
            XCTAssertEqual(source.url, spec.url, "\(spec.name): url")
            XCTAssertEqual(urlByName[spec.name], spec.url, "\(spec.name): const URL")
            XCTAssertEqual(
                constants.cacheFileNamePattern.replacingOccurrences(of: "{name}", with: spec.name),
                spec.cacheFileName, "\(spec.name): cacheFileName pattern")
            // compare the production filename formula (cacheFileName), not just
            // the fixture pattern, so the real formula cannot drift unseen
            XCTAssertEqual(cacheFileName(spec.name), spec.cacheFileName, "\(spec.name): cacheFileName formula")
        }

        XCTAssertEqual(Pricing.defaultTtlMs, constants.defaultTtlMs, "defaultTtlMs")
        // the facade keeps the fetch timeout in seconds; the golden value is in ms
        XCTAssertEqual(Int64(fetchTimeout * 1000), constants.fetchTimeoutMs, "fetchTimeoutMs")
    }

    // MARK: - probe contract vs shared gold

    /// The shared unusable-payload probe contract (probe/cases.json): the
    /// facade's own probe (Ffi.probeModelCount, the same internal function its
    /// lifecycle uses) must report usable/unusable exactly per case. An
    /// invalid-JSON payloadRaw is fed as the raw payload bytes verbatim; the
    /// native build parses it to zero.
    func testProbeConformance() throws {
        let url = repoRoot()
            .appendingPathComponent("golden")
            .appendingPathComponent("pricing")
            .appendingPathComponent("probe")
            .appendingPathComponent("cases.json")
        let cases = try JSONSerialization.jsonObject(with: Data(contentsOf: url)) as! [[String: Any]]
        for kase in cases {
            let name = kase["name"] as! String
            let format = kase["format"] as! String
            let usable = kase["usable"] as! Bool
            let payload: Data
            if let raw = kase["payloadRaw"] as? String {
                payload = Data(raw.utf8)
            } else {
                payload = try JSONSerialization.data(withJSONObject: kase["payload"]!)
            }
            let got = Ffi.probeModelCount(source: "probe", format: format, payload: payload) > 0
            XCTAssertEqual(got, usable, name)
        }
    }

    // MARK: - helpers

    private func normalized<T: Encodable>(_ value: T) -> JSONAny {
        let data = try! JSONEncoder().encode(value)
        return try! JSONDecoder().decode(JSONAny.self, from: data)
    }
}
