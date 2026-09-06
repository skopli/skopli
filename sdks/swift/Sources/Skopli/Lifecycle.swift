import Foundation

#if canImport(FoundationNetworking)
    import FoundationNetworking
#endif

#if os(Windows)
    import WinSDK
#elseif canImport(Glibc)
    import Glibc
#elseif canImport(Darwin)
    import Darwin
#endif

// The facade-side built-in pricing lifecycle (mirroring src/pricing/sources.ts
// and src/pricing/cache.ts). Built-in sources fetch a raw payload, cache it
// byte-compatibly with the TS cache format, and serve the cache within TTL,
// offline, or as a stale fallback when a fetch fails. Parsers are never
// reimplemented: raw bytes go to the core parser via the catalog format+payload
// path with `builtinSources: false`. No callback ever crosses the FFI.

/// Built-in source endpoints (mirroring src/pricing/sources.ts).
public enum BuiltinSourceURL {
    public static let openRouter = "https://openrouter.ai/api/v1/models"
    public static let liteLlm =
        "https://raw.githubusercontent.com/BerriAI/litellm/main/model_prices_and_context_window.json"
    public static let modelsDev = "https://models.dev/api.json"
}

let fetchTimeout: TimeInterval = 10

/// The default `URLSession` getter: a stalled endpoint must never hang an
/// otherwise local run, so it carries a bounded timeout. Non-2xx responses and
/// transport errors throw so the caller falls back to a stale cache.
@usableFromInline let defaultFetch: @Sendable (URL) throws -> Data = { url in
    let semaphore = DispatchSemaphore(value: 0)
    var result: Result<Data, Error> = .failure(
        SkopliError.internalError("fetch produced no result"))
    var request = URLRequest(url: url)
    request.timeoutInterval = fetchTimeout
    let task = URLSession.shared.dataTask(with: request) { data, response, error in
        defer { semaphore.signal() }
        if let error {
            result = .failure(error)
            return
        }
        if let http = response as? HTTPURLResponse, !(200..<300).contains(http.statusCode) {
            result = .failure(SkopliError.internalError("\(url) responded \(http.statusCode)"))
            return
        }
        result = .success(data ?? Data())
    }
    task.resume()
    semaphore.wait()
    return try result.get()
}

/// The default priority-ordered market sources: OpenRouter > LiteLLM >
/// models.dev.
func builtinSources() -> [PricingSource] {
    [
        openRouterSource(),
        liteLlmSource(),
        modelsDevSource(),
    ]
}

/// The built-in OpenRouter market source.
public func openRouterSource(url: String = BuiltinSourceURL.openRouter) -> PricingSource {
    CachedSource(name: "openrouter", format: "openrouter", url: url)
}

/// The built-in LiteLLM market source.
public func liteLlmSource(url: String = BuiltinSourceURL.liteLlm) -> PricingSource {
    CachedSource(name: "litellm", format: "litellm", url: url)
}

/// The built-in models.dev market source.
public func modelsDevSource(url: String = BuiltinSourceURL.modelsDev) -> PricingSource {
    CachedSource(name: "models-dev", format: "modelsdev", url: url)
}

/// The on-disk cache filename a built-in source reads and writes; the single
/// production formula, exposed for the constants contract test in golden/pricing/lifecycle.
func cacheFileName(_ name: String) -> String {
    "pricing-\(name).json"
}

/// A built-in market source: fetches a raw payload, caches it byte-compatibly
/// with the TS cache format, and serves the cache within TTL, offline, or as a
/// stale fallback when a fetch fails. It never parses the payload; the raw bytes
/// are handed to the core parser via the catalog format+payload path.
struct CachedSource: PricingSource {
    let name: String
    let format: String
    let url: String

    func load(context: SourceContext) throws -> Catalog? {
        let cacheName = cacheFileName(name)
        let cached = loadCached(context.cacheDir, cacheName, context.ttlMs, context.now())
        // A cache payload that parses to zero prices is unusable, same as no
        // cache. Validation goes through the core parser via a native probe,
        // never a facade-side reimplementation of the source parsers.
        func catalog() -> Catalog? {
            guard let cached else { return nil }
            guard Ffi.probeModelCount(source: name, format: format, payload: cached.payload) > 0
            else { return nil }
            return Catalog(
                source: name, fetchedAt: cached.fetchedAt, format: format,
                payload: RawJSON(cached.payload))
        }
        let usableCached = catalog()
        let cacheFresh = cached != nil && !cached!.stale && !context.refresh
        if let c = usableCached, cacheFresh || context.offline {
            return c
        }
        if context.offline { return usableCached }
        let payload: Data
        do {
            payload = try context.fetch(URL(string: url)!)
        } catch {
            // A fetch failure (transport, non-2xx) serves the stale/prior cached
            // catalog without touching the cache file.
            return usableCached
        }
        // A fetched payload that is invalid JSON or parses to zero prices is
        // treated as a fetch failure: never write the cache, serve stale.
        if Ffi.probeModelCount(source: name, format: format, payload: payload) == 0 {
            return usableCached
        }
        let fetchedAt = try storeCached(context.cacheDir, cacheName, payload, context.now())
        return Catalog(source: name, fetchedAt: fetchedAt, format: format, payload: RawJSON(payload))
    }
}

/// A cache read result: the raw payload, its stamp, and whether its age exceeds
/// the TTL (age > ttlMs is stale; age == ttlMs is fresh).
struct CachedPayload {
    let fetchedAt: String
    let payload: Data
    let stale: Bool
}

/// The on-disk cache body, byte-compatible with src/pricing/cache.ts:
/// {"fetchedAt": "<ISO-8601 UTC>", "payload": <raw source JSON>}.
private struct CacheFile: Codable {
    let fetchedAt: String
    let payload: RawJSON
}

/// Read a cache file, returning nil when it is absent, malformed, or carries an
/// unparseable timestamp. stale is age > ttlMs.
func loadCached(_ cacheDir: String, _ name: String, _ ttlMs: Int64, _ now: Date) -> CachedPayload? {
    let path = URL(fileURLWithPath: cacheDir).appendingPathComponent(name)
    guard let raw = try? Data(contentsOf: path) else { return nil }
    guard let file = try? JSONDecoder().decode(CacheFile.self, from: raw) else { return nil }
    guard let fetched = parseISO(file.fetchedAt) else { return nil }
    let age = Int64((now.timeIntervalSince1970 - fetched.timeIntervalSince1970) * 1000)
    return CachedPayload(fetchedAt: file.fetchedAt, payload: file.payload.data, stale: age > ttlMs)
}

/// Write a cache file via a temp sibling then rename (replaceItemAt is
/// unimplemented in corelibs-foundation on Windows), stamped with `now` as an
/// ISO-8601 UTC timestamp, and return that stamp.
func storeCached(_ cacheDir: String, _ name: String, _ payload: Data, _ now: Date) throws -> String
{
    let fetchedAt = isoUTC(now)
    let body: Data
    do {
        body = try JSONEncoder().encode(CacheFile(fetchedAt: fetchedAt, payload: RawJSON(payload)))
    } catch {
        throw SkopliError.internalError("encoding cache file: \(error)")
    }
    let fm = FileManager.default
    let dir = URL(fileURLWithPath: cacheDir)
    do {
        try fm.createDirectory(at: dir, withIntermediateDirectories: true)
    } catch {
        throw SkopliError.internalError("creating cache dir: \(error)")
    }
    let target = dir.appendingPathComponent(name)
    let tmp = dir.appendingPathComponent("\(name).\(ProcessInfo.processInfo.processIdentifier).\(UInt64.random(in: 0..<UInt64.max)).tmp")
    do {
        try body.write(to: tmp)
        // The single overwrite-capable rename is the only commit point; the old
        // target survives until it succeeds (replaceItemAt is unimplemented in
        // corelibs-foundation on Windows, and remove-then-move is not crash-safe).
        try atomicReplace(from: tmp, to: target)
    } catch {
        try? fm.removeItem(at: tmp)
        throw SkopliError.internalError("writing cache file: \(error)")
    }
    return fetchedAt
}

/// Atomically replace `to` with `from` on the same filesystem: MoveFileExW with
/// MOVEFILE_REPLACE_EXISTING on Windows, POSIX rename(2) elsewhere.
private func atomicReplace(from: URL, to: URL) throws {
    #if os(Windows)
        let ok = from.path.withCString(encodedAs: UTF16.self) { src in
            to.path.withCString(encodedAs: UTF16.self) { dst in
                MoveFileExW(src, dst, DWORD(MOVEFILE_REPLACE_EXISTING))
            }
        }
        if !ok {
            throw SkopliError.internalError("MoveFileExW failed: \(GetLastError())")
        }
    #else
        let rc = from.withUnsafeFileSystemRepresentation { src in
            to.withUnsafeFileSystemRepresentation { dst in
                rename(src, dst)
            }
        }
        if rc != 0 {
            throw SkopliError.internalError("rename failed: errno \(errno)")
        }
    #endif
}

/// Render `date` as an ISO-8601 UTC timestamp with millisecond precision,
/// matching the TS Date.toISOString() format the cache format pins.
func isoUTC(_ date: Date) -> String {
    // Round the absolute timestamp to integral milliseconds FIRST, rebuild the
    // Date from that value, then format the calendar components from the rebuilt
    // Date so the second and millisecond fields never disagree (no ".1000" and
    // no negative field near a boundary or pre-epoch).
    let ms = Int64((date.timeIntervalSince1970 * 1000).rounded())
    let d = Date(timeIntervalSince1970: Double(ms) / 1000)
    let msField = Int(((ms % 1000) + 1000) % 1000)
    var c = Calendar(identifier: .gregorian)
    c.timeZone = TimeZone(identifier: "UTC")!
    let comps = c.dateComponents([.year, .month, .day, .hour, .minute, .second], from: d)
    return String(
        format: "%04d-%02d-%02dT%02d:%02d:%02d.%03dZ",
        comps.year!, comps.month!, comps.day!, comps.hour!, comps.minute!, comps.second!, msField)
}

/// Parse an ISO-8601 UTC timestamp with millisecond precision.
private func parseISO(_ text: String) -> Date? {
    let formatter = DateFormatter()
    formatter.locale = Locale(identifier: "en_US_POSIX")
    formatter.timeZone = TimeZone(identifier: "UTC")
    formatter.dateFormat = "yyyy-MM-dd'T'HH:mm:ss.SSSZZZZZ"
    if let d = formatter.date(from: text) { return d }
    formatter.dateFormat = "yyyy-MM-dd'T'HH:mm:ssZZZZZ"
    return formatter.date(from: text)
}
