import CSkopli
import Foundation

// The ONLY file touching the C ABI (CSkopli). It owns every AgBuf /
// C-string lifetime (library-owned outputs are copied into Swift Data/String and
// freed via ag_buf_free / ag_string_free), the (ptr, len) input borrowing, and
// the AgStatus -> SkopliError mapping (carrying the thread-local
// ag_last_error_message detail). Everything above it is pure Swift.
//
// Signature parity is verified character-by-character against
// crates/skopli-capi/include/skopli.h (see 20-REPORT.md parity table).
// AgStatus is a C enum -> imported as a Swift type whose `.rawValue` is Int32;
// AgBuf is `{ uint8_t *ptr; uintptr_t len; }`; AgPricing is opaque
// (`OpaquePointer`). uintptr_t maps to Swift `UInt`.

enum Ffi {
    // MARK: - Status codes

    // The AgStatus contract (skopli.h: enum AgStatus). The
    // header typedefs AgStatus to int32_t on this C standard, so every fallible
    // fn returns a plain Int32. We compare against these literals directly rather
    // than the imported enum constants to sidestep the C-enum-vs-typedef import
    // ambiguity (the numeric values ARE the ABI contract, checked in the parity
    // table).
    private static let statusOk: Int32 = 0
    private static let statusInvalidArgument: Int32 = 1
    private static let statusCatalog: Int32 = 2
    private static let statusInternal: Int32 = 3

    // MARK: - Status -> error mapping

    /// Read (and free) this thread's last native error detail, or "" when none.
    private static func lastErrorMessage() -> String {
        var out: UnsafeMutablePointer<CChar>? = nil
        // ag_last_error_message(char** out) -> AgStatus
        guard ag_last_error_message(&out) == statusOk, let out else {
            return ""
        }
        defer { ag_string_free(out) }
        return String(cString: out)
    }

    /// Turn a non-OK AgStatus into the mapped SkopliError, carrying the
    /// thread-local detail message.
    private static func error(for status: Int32) -> SkopliError {
        let detail = lastErrorMessage()
        switch status {
        case statusInvalidArgument: return .invalidArgument(detail)
        case statusCatalog: return .catalog(detail)
        default: return .internalError(detail)
        }
    }

    // MARK: - Buffer ownership

    /// Take ownership of an out-`AgBuf`, copy it into `Data`, and free it. A zero
    /// buffer (`ptr == nil` / `len == 0`) yields empty `Data`.
    private static func take(_ buf: AgBuf) -> Data {
        defer { ag_buf_free(buf) }
        guard let ptr = buf.ptr, buf.len > 0 else { return Data() }
        return Data(bytes: ptr, count: Int(buf.len))
    }

    /// Lend a nullable (ptr, len) view of `Data` to a C ABI call. `nil`/empty
    /// becomes `(nil, 0)` - the ABI's "null options = all defaults" contract.
    /// The pointer is valid only for the duration of `body`.
    private static func withOptBytes<R>(
        _ data: Data?,
        _ body: (UnsafePointer<CChar>?, UInt) throws -> R
    ) rethrows -> R {
        guard let data, !data.isEmpty else {
            return try body(nil, 0)
        }
        return try data.withUnsafeBytes { raw in
            let base = raw.baseAddress!.assumingMemoryBound(to: CChar.self)
            return try body(base, UInt(raw.count))
        }
    }

    // MARK: - Meta / lifecycle

    static func abiVersion() -> UInt32 { ag_abi_version() }
    static func schemaVersion() -> UInt32 { ag_schema_version() }

    static func version() -> String {
        // ag_version() -> const char* (static; NEVER freed).
        guard let p = ag_version() else { return "" }
        return String(cString: p)
    }

    static func defaultCacheDir() throws -> String {
        // ag_default_cache_dir(char** out) -> AgStatus
        var out: UnsafeMutablePointer<CChar>? = nil
        let status = ag_default_cache_dir(&out)
        guard status == statusOk else { throw error(for: status) }
        guard let out else { return "" }
        defer { ag_string_free(out) }
        return String(cString: out)
    }

    // MARK: - Pipeline (JSON in / JSON out)

    /// ag_detect_harnesses(const char* opts_json, uintptr_t opts_len, AgBuf* out)
    static func detectHarnesses(_ opts: Data?) throws -> Data {
        try withOptBytes(opts) { ptr, len in
            var out = AgBuf()
            let status = ag_detect_harnesses(ptr, len, &out)
            guard status == statusOk else { throw error(for: status) }
            return take(out)
        }
    }

    /// ag_read_usage(const char* opts_json, uintptr_t opts_len, AgBuf* out)
    static func readUsage(_ opts: Data?) throws -> Data {
        try withOptBytes(opts) { ptr, len in
            var out = AgBuf()
            let status = ag_read_usage(ptr, len, &out)
            guard status == statusOk else { throw error(for: status) }
            return take(out)
        }
    }

    /// ag_rollup(const char* events_json, uintptr_t events_len,
    ///           const char* opts_json, uintptr_t opts_len, AgBuf* out)
    static func rollup(events: Data, opts: Data) throws -> Data {
        try withOptBytes(events) { ePtr, eLen in
            try withOptBytes(opts) { oPtr, oLen in
                var out = AgBuf()
                let status = ag_rollup(ePtr, eLen, oPtr, oLen, &out)
                guard status == statusOk else { throw error(for: status) }
                return take(out)
            }
        }
    }

    /// ag_cost_usd(const char* tokens_json, uintptr_t tokens_len,
    ///             const char* price_json, uintptr_t price_len, double* out)
    static func costUsd(tokens: Data, price: Data) throws -> Double {
        try withOptBytes(tokens) { tPtr, tLen in
            try withOptBytes(price) { pPtr, pLen in
                var out: Double = 0
                let status = ag_cost_usd(tPtr, tLen, pPtr, pLen, &out)
                guard status == statusOk else { throw error(for: status) }
                return out
            }
        }
    }

    // MARK: - Pricing handle

    /// ag_pricing_new(const char* opts_json, uintptr_t opts_len, AgPricing** out)
    /// Returns the opaque handle; the caller owns it and must ag_pricing_free it.
    static func pricingNew(_ opts: Data?) throws -> OpaquePointer {
        try withOptBytes(opts) { ptr, len in
            var handle: OpaquePointer? = nil
            let status = ag_pricing_new(ptr, len, &handle)
            guard status == statusOk else { throw error(for: status) }
            guard let handle else { throw error(for: statusInternal) }
            return handle
        }
    }

    /// ag_pricing_free(AgPricing* pricing) - null is a safe no-op.
    static func pricingFree(_ handle: OpaquePointer?) {
        ag_pricing_free(handle)
    }

    /// Probe a single raw source payload through the core parser: build a
    /// throwaway native handle with exactly one raw catalog
    /// (`{source, format, payload}`, `builtinSources: false`, mode `calculate`),
    /// read that catalog's `models` count, and free the handle. Returns the model
    /// count, or 0 when the build fails or the payload parses to no prices. Never
    /// reimplements a source parser.
    static func probeModelCount(source: String, format: String, payload: Data) -> Int {
        // Assemble the throwaway options JSON with the raw payload embedded
        // verbatim (it is already valid JSON bytes, or invalid - in which case the
        // build fails and we report 0).
        var opts = Data()
        opts.append(contentsOf: #"{"mode":"calculate","builtinSources":false,"catalogs":[{"source":"#.utf8)
        opts.append(contentsOf: encodeJSONString(source).utf8)
        opts.append(contentsOf: #","format":"#.utf8)
        opts.append(contentsOf: encodeJSONString(format).utf8)
        opts.append(contentsOf: #","payload":"#.utf8)
        opts.append(payload)
        opts.append(contentsOf: "}]}".utf8)

        guard let handle = try? pricingNew(opts) else { return 0 }
        defer { pricingFree(handle) }
        guard let out = try? catalogInfo(handle) else { return 0 }
        guard
            let root = try? JSONSerialization.jsonObject(with: out) as? [[String: Any]]
        else { return 0 }
        for info in root {
            if info["source"] as? String == source, let models = info["models"] as? NSNumber {
                return models.intValue
            }
        }
        return 0
    }

    /// Encode a Swift string as a JSON string literal (including quotes).
    private static func encodeJSONString(_ s: String) -> String {
        let data = try? JSONEncoder().encode(s)
        return data.flatMap { String(data: $0, encoding: .utf8) } ?? "\"\""
    }

    /// ag_pricing_price_events(const AgPricing* pricing, const char* events_json,
    ///   uintptr_t events_len, const char* opts_json, uintptr_t opts_len,
    ///   AgBuf* out)
    static func priceEvents(_ handle: OpaquePointer, events: Data, opts: Data) throws -> Data {
        try withOptBytes(events) { ePtr, eLen in
            try withOptBytes(opts) { oPtr, oLen in
                var out = AgBuf()
                let status = ag_pricing_price_events(handle, ePtr, eLen, oPtr, oLen, &out)
                guard status == statusOk else { throw error(for: status) }
                return take(out)
            }
        }
    }

    /// ag_pricing_catalog_info(const AgPricing* pricing, AgBuf* out)
    static func catalogInfo(_ handle: OpaquePointer) throws -> Data {
        var out = AgBuf()
        let status = ag_pricing_catalog_info(handle, &out)
        guard status == statusOk else { throw error(for: status) }
        return take(out)
    }

    /// ag_pricing_price_rollups(const AgPricing* pricing, const char* rollups_json,
    ///   uintptr_t rollups_len, AgBuf* out)
    static func priceRollups(_ handle: OpaquePointer, rollups: Data) throws -> Data {
        try withOptBytes(rollups) { ptr, len in
            var out = AgBuf()
            let status = ag_pricing_price_rollups(handle, ptr, len, &out)
            guard status == statusOk else { throw error(for: status) }
            return take(out)
        }
    }

    /// ag_pricing_lookup_model(const AgPricing* pricing, const char* model,
    ///   uintptr_t model_len, AgBuf* out) - `model` is a bare UTF-8 name, not JSON.
    static func lookupModel(_ handle: OpaquePointer, model: Data) throws -> Data {
        try withOptBytes(model) { ptr, len in
            var out = AgBuf()
            let status = ag_pricing_lookup_model(handle, ptr, len, &out)
            guard status == statusOk else { throw error(for: status) }
            return take(out)
        }
    }
}
