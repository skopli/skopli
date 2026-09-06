import Foundation

// A minimal type-erased JSON value used only to round-trip a raw JSON fragment
// (Catalog.payload) verbatim through Codable. The catalog payload is an
// arbitrary raw source snapshot (openrouter/litellm/models.dev JSON); Swift's
// singleValueContainer cannot emit raw bytes, so RawJSON decodes the fragment to
// a JSONAny and re-encodes it, preserving structure. Object key order is not
// preserved, which is irrelevant to the core parsers (they key by name).

enum JSONAny: Codable, Equatable {
    case null
    case bool(Bool)
    case int(Int64)
    case double(Double)
    case string(String)
    case array([JSONAny])
    case object([String: JSONAny])

    init(from decoder: Decoder) throws {
        let container = try decoder.singleValueContainer()
        if container.decodeNil() {
            self = .null
        } else if let v = try? container.decode(Bool.self) {
            self = .bool(v)
        } else if let v = try? container.decode(Int64.self) {
            self = .int(v)
        } else if let v = try? container.decode(Double.self) {
            self = .double(v)
        } else if let v = try? container.decode(String.self) {
            self = .string(v)
        } else if let v = try? container.decode([JSONAny].self) {
            self = .array(v)
        } else if let v = try? container.decode([String: JSONAny].self) {
            self = .object(v)
        } else {
            throw DecodingError.dataCorruptedError(
                in: container, debugDescription: "unsupported JSON value")
        }
    }

    func encode(to encoder: Encoder) throws {
        var container = encoder.singleValueContainer()
        switch self {
        case .null: try container.encodeNil()
        case .bool(let v): try container.encode(v)
        case .int(let v): try container.encode(v)
        case .double(let v): try container.encode(v)
        case .string(let v): try container.encode(v)
        case .array(let v): try container.encode(v)
        case .object(let v): try container.encode(v)
        }
    }
}
