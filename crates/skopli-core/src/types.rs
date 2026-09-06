use serde::Serialize;

/// Token counters for a single usage event. Mirrors `TokenCounts` in
/// src/types.ts. Counters are `u64` by contract (integers, never floats).
/// `cache_write1h` is absent when the source omits the split.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct TokenCounts {
    pub input: u64,
    pub output: u64,
    #[serde(rename = "cacheRead")]
    pub cache_read: u64,
    #[serde(rename = "cacheWrite")]
    pub cache_write: u64,
    #[serde(rename = "cacheWrite1h", skip_serializing_if = "Option::is_none")]
    pub cache_write1h: Option<u64>,
    pub reasoning: u64,
}

/// A single normalized usage event. Mirrors `UsageEvent` in src/types.ts,
/// with fields declared in the same order so serialized key order matches the
/// gold-file grammar (the comparator ignores key order regardless).
///
/// Optional fields are OMITTED when `None` (never serialized as `null`) per
/// the gold-file contract. `cost_usd` is a pass-through `f64` (spike A: the
/// claude reader never re-serializes it).
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct UsageEvent {
    pub harness: String,
    pub timestamp: String,
    #[serde(rename = "sessionId")]
    pub session_id: String,
    #[serde(rename = "messageId")]
    pub message_id: String,
    pub turn: bool,
    pub subagent: bool,
    pub model: String,
    pub tokens: TokenCounts,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub calls: Option<u64>,
    #[serde(rename = "costUsd", skip_serializing_if = "Option::is_none")]
    pub cost_usd: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub workspace: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
}
