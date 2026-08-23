use serde_json::Value;

use super::shared::{finite_number, is_record};
use crate::types::TokenCounts;

/// The set of source field names parsed for a gemini-family token record.
/// Mirrors `GeminiTokenFields` in src/readers/gemini-core.ts.
pub struct GeminiTokenFields {
    pub input: &'static str,
    pub output: &'static str,
    pub cache_read: &'static str,
    pub reasoning: &'static str,
    pub tool: Option<&'static str>,
    pub total: Option<&'static str>,
    pub optional: bool,
}

/// Default gemini field mapping (`geminiTokenFields`).
pub const GEMINI_TOKEN_FIELDS: GeminiTokenFields = GeminiTokenFields {
    input: "input",
    output: "output",
    cache_read: "cached",
    reasoning: "thoughts",
    tool: Some("tool"),
    total: Some("total"),
    optional: false,
};

/// Qwen field mapping (`qwenTokenFields`): optional-mode with the Gemini API's
/// `*TokenCount` field names and no tool/total inclusivity split.
pub const QWEN_TOKEN_FIELDS: GeminiTokenFields = GeminiTokenFields {
    input: "promptTokenCount",
    output: "candidatesTokenCount",
    cache_read: "cachedContentTokenCount",
    reasoning: "thoughtsTokenCount",
    tool: None,
    total: None,
    optional: true,
};

/// Parse a gemini token record. Faithful port of `parseGeminiTokens`. Returns
/// `None` when the record is malformed for the given field mapping.
pub fn parse_gemini_tokens(value: &Value, fields: &GeminiTokenFields) -> Option<TokenCounts> {
    if !is_record(value) {
        return None;
    }
    let field = |name: &str| value.get(name).and_then(finite_number);
    let input = field(fields.input);
    let output = field(fields.output);
    let cache_read = field(fields.cache_read);

    if fields.optional {
        let optional_fields = [
            fields.input,
            fields.output,
            fields.cache_read,
            fields.reasoning,
        ];
        if optional_fields.iter().any(|name| {
            value.get(*name).is_some() && value.get(*name).and_then(finite_number).is_none()
        }) {
            return None;
        }
        return Some(TokenCounts {
            input: input.unwrap_or(0.0).max(0.0) as u64,
            output: output.unwrap_or(0.0).max(0.0) as u64,
            cache_read: cache_read.unwrap_or(0.0).max(0.0) as u64,
            cache_write: 0,
            cache_write1h: None,
            reasoning: field(fields.reasoning).unwrap_or(0.0).max(0.0) as u64,
        });
    }

    let (input, output, cache_read) = match (input, output, cache_read) {
        (Some(i), Some(o), Some(c)) => (i, o, c),
        _ => return None,
    };
    let reasoning = field(fields.reasoning).unwrap_or(0.0);
    let tool = fields.tool.and_then(field).unwrap_or(0.0);
    // gemini chat recordings are ambiguous about whether input includes cached
    // tokens; subtract only when the total field proves inclusivity
    let total = fields.total.and_then(field);
    let cache_inclusive = matches!(total, Some(t) if t == input + output + reasoning + tool);
    let resolved_input = if cache_inclusive {
        (input - cache_read.min(input)).max(0.0)
    } else {
        input
    } + tool;
    Some(TokenCounts {
        input: resolved_input as u64,
        output: output as u64,
        cache_read: cache_read as u64,
        cache_write: 0,
        cache_write1h: None,
        reasoning: reasoning as u64,
    })
}
