import Foundation

/// The typed error surfaced by every fallible Skopli operation. Cases map
/// 1:1 from the C ABI `AgStatus` codes (1 invalid-argument,
/// 2 catalog, 3 internal); the associated string carries the thread-local
/// `ag_last_error_message` detail. A JSON decode failure on a well-formed ABI
/// response is reported as `.internalError` (the ABI produced malformed output).
///
/// The core never throws for malformed *data* (that surfaces in
/// `diagnostics`/`skipped`); errors here mean a programmer mistake (bad
/// date/tz/JSON, invalid catalog) or an I/O-fatal/internal condition.
public enum SkopliError: Error, Equatable, CustomStringConvertible {
    /// `AG_STATUS_INVALID_ARGUMENT` (1): a bad argument - null where required,
    /// malformed JSON, an out-of-range date/tz, or a bad enum value.
    case invalidArgument(String)
    /// `AG_STATUS_CATALOG` (2): a pricing/catalog operation failed (e.g.
    /// unparseable catalog JSON).
    case catalog(String)
    /// `AG_STATUS_INTERNAL` (3): an unexpected internal error (including a caught
    /// panic in the native library), or a failure decoding a native response.
    case internalError(String)

    public var description: String {
        switch self {
        case .invalidArgument(let m): return "skopli: invalid argument: \(m)"
        case .catalog(let m): return "skopli: catalog error: \(m)"
        case .internalError(let m): return "skopli: internal error: \(m)"
        }
    }
}
