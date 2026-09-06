//! Tiered model matcher. Faithful port of `src/pricing/match.ts`.
//!
//! Candidate keys are tried against a catalog in decreasing confidence:
//!   1. exact name
//!   2. provider-prefix strip (each shorter suffix of a multi-segment path)
//!      and vendor route-prefix strip (`us.anthropic.claude-...` ->
//!      `anthropic.claude-...`)
//!   3. family pattern aliases (`anthropic.claude-sonnet-4-5-...-v1:0` ->
//!      `claude-sonnet-4.5`)
//!   4. separator normalization (dots vs dashes) and trailing date-stamp strip
//!   5. provider-prefixed forms of a family key (`claude-*` ->
//!      `anthropic/claude-*`)
//!   6. alias table
//!
//! The TS regexes are reproduced by hand (the core deliberately carries no
//! `regex` dependency, matching the rest of the reader layer).

use std::collections::HashSet;

use super::types::{ModelPrice, PriceMap};

/// Vendor routing prefixes that wrap an underlying id inside a single name
/// segment (Bedrock cross-region inference profiles prepend a geographic
/// prefix, so `us.anthropic.claude-...` carries `anthropic.claude-...`).
const ROUTE_PREFIXES: &[&str] = &["us.", "eu.", "apac.", "global."];

/// Apply the family rebrand pattern
/// `^anthropic\.(claude-[a-z]+)-(\d+)-(\d+)(?:-\d{8})?-v\d+(?::\d+)?$`
/// -> `$1-$2.$3`, returning the alias when `variant` matches (Amazon Bedrock
/// keys Claude as `anthropic.claude-sonnet-4-5-20250929-v1:0`).
fn family_alias(variant: &str) -> Option<String> {
    let rest = variant.strip_prefix("anthropic.claude-")?;
    // `[a-z]+` family word then `-`
    let fam_end = rest.find(|c: char| !c.is_ascii_lowercase())?;
    if fam_end == 0 || rest.as_bytes().get(fam_end) != Some(&b'-') {
        return None;
    }
    let fam = &rest[..fam_end];
    let after_fam = &rest[fam_end + 1..]; // skip family word and its trailing '-'
    // `(\d+)-(\d+)`
    let (major, tail) = after_fam.split_once('-')?;
    if major.is_empty() || !major.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    // minor is the leading run of digits in `tail`; the rest must be an
    // optional `-YYYYMMDD` date stamp then `-vN` or `-vN:M`
    let minor_end = tail
        .find(|c: char| !c.is_ascii_digit())
        .unwrap_or(tail.len());
    if minor_end == 0 {
        return None;
    }
    let minor = &tail[..minor_end];
    let mut suffix = &tail[minor_end..];
    // optional `-YYYYMMDD`
    if let Some(after_date) = suffix.strip_prefix('-')
        && after_date.len() >= 8
        && after_date.as_bytes()[..8].iter().all(u8::is_ascii_digit)
    {
        let is_version_boundary = after_date.as_bytes().get(8);
        if is_version_boundary == Some(&b'-') {
            suffix = &after_date[8..];
        }
    }
    // required `-vN` with an optional `:M`
    let ver = suffix.strip_prefix("-v")?;
    let ver_digits_end = ver.find(|c: char| !c.is_ascii_digit()).unwrap_or(ver.len());
    if ver_digits_end == 0 {
        return None;
    }
    let after_ver = &ver[ver_digits_end..];
    if !after_ver.is_empty() {
        // only `:` then a run of digits may follow
        let colon_rest = after_ver.strip_prefix(':')?;
        if colon_rest.is_empty() || !colon_rest.bytes().all(|b| b.is_ascii_digit()) {
            return None;
        }
    }
    Some(format!("claude-{fam}-{major}.{minor}"))
}

/// Reproduce `variant.replace(/(\d)-(\d)/g, "$1.$2")`: every `-` sitting between
/// two ASCII digits becomes `.`. The JS global replace does not re-scan a
/// consumed digit, so overlapping matches share a digit exactly as JS does.
fn digit_dash_to_dot(variant: &str) -> String {
    let bytes = variant.as_bytes();
    let mut out = String::with_capacity(variant.len());
    let mut i = 0;
    while i < bytes.len() {
        // A match is digit, '-', digit starting at i; JS consumes all three and
        // resumes after the second digit.
        if i + 2 < bytes.len()
            && bytes[i].is_ascii_digit()
            && bytes[i + 1] == b'-'
            && bytes[i + 2].is_ascii_digit()
        {
            out.push(bytes[i] as char);
            out.push('.');
            out.push(bytes[i + 2] as char);
            i += 3;
        } else {
            out.push(bytes[i] as char);
            i += 1;
        }
    }
    out
}

/// Reproduce
/// `variant.replace(/-\d{8}$/, "").replace(/-\d{4}-\d{2}-\d{2}$/, "")`:
/// strip a trailing `-YYYYMMDD` first, then (on the result) a trailing
/// `-YYYY-MM-DD`. The two `.replace` calls are sequential, so both can apply.
fn strip_trailing_date(variant: &str) -> String {
    let step1 = strip_suffix_8_digits(variant);
    strip_suffix_iso_date(&step1)
}

/// Strip a trailing `-` followed by exactly 8 ASCII digits at end of string.
fn strip_suffix_8_digits(s: &str) -> String {
    let bytes = s.as_bytes();
    if bytes.len() >= 9 {
        let tail = &bytes[bytes.len() - 9..];
        if tail[0] == b'-' && tail[1..].iter().all(|b| b.is_ascii_digit()) {
            return s[..s.len() - 9].to_owned();
        }
    }
    s.to_owned()
}

/// Strip a trailing `-YYYY-MM-DD` (`-\d{4}-\d{2}-\d{2}$`) at end of string.
fn strip_suffix_iso_date(s: &str) -> String {
    let bytes = s.as_bytes();
    // length of "-YYYY-MM-DD" == 11
    if bytes.len() >= 11 {
        let tail = &bytes[bytes.len() - 11..];
        let digits_at = |idx: usize| tail[idx].is_ascii_digit();
        if tail[0] == b'-'
            && digits_at(1)
            && digits_at(2)
            && digits_at(3)
            && digits_at(4)
            && tail[5] == b'-'
            && digits_at(6)
            && digits_at(7)
            && tail[8] == b'-'
            && digits_at(9)
            && digits_at(10)
        {
            return s[..s.len() - 11].to_owned();
        }
    }
    s.to_owned()
}

/// True when `variant` matches `^claude-` (the only provider-prefix pattern),
/// yielding the `anthropic/`-prefixed form.
fn provider_prefixed(variant: &str) -> Option<String> {
    if variant.starts_with("claude-") {
        Some(format!("anthropic/{variant}"))
    } else {
        None
    }
}

/// Candidate keys to try against a catalog, in decreasing confidence. Faithful
/// port of `candidateKeys` in src/pricing/match.ts, preserving push order and
/// the de-dup-on-push behaviour (`!keys.includes(key)`).
pub fn candidate_keys(model: &str) -> Vec<String> {
    let mut keys: Vec<String> = Vec::new();
    let push = |keys: &mut Vec<String>, key: String| {
        if !key.is_empty() && !keys.contains(&key) {
            keys.push(key);
        }
    };

    push(&mut keys, model.to_owned());

    let segments: Vec<&str> = model.split('/').collect();
    for i in 1..segments.len() {
        push(&mut keys, segments[i..].join("/"));
    }

    let bare = *segments.last().unwrap_or(&model);

    // `variants` is a JS Set seeded with {model, bare}. Insertion order is
    // preserved (Set iteration order) and each `expand` extends it in place.
    let mut variants: Vec<String> = Vec::new();
    let mut variants_seen: HashSet<String> = HashSet::new();
    let add_variant = |variants: &mut Vec<String>, seen: &mut HashSet<String>, v: String| {
        if seen.insert(v.clone()) {
            variants.push(v);
        }
    };
    add_variant(&mut variants, &mut variants_seen, model.to_owned());
    add_variant(&mut variants, &mut variants_seen, bare.to_owned());

    // Each `expand` snapshots the current variants, derives from each, and adds
    // the results (Set de-dups), exactly as the TS `expand` helper does.
    let expand = |variants: &mut Vec<String>,
                  seen: &mut HashSet<String>,
                  derive: &dyn Fn(&str) -> Vec<String>| {
        let snapshot: Vec<String> = variants.clone();
        for variant in &snapshot {
            for derived in derive(variant) {
                if seen.insert(derived.clone()) {
                    variants.push(derived);
                }
            }
        }
    };

    // route-prefix strip
    expand(&mut variants, &mut variants_seen, &|variant| {
        ROUTE_PREFIXES
            .iter()
            .filter(|prefix| variant.starts_with(*prefix))
            .map(|prefix| variant[prefix.len()..].to_owned())
            .collect()
    });
    // family pattern aliases
    expand(&mut variants, &mut variants_seen, &|variant| {
        family_alias(variant).into_iter().collect()
    });
    // separator normalization: replaceAll('.', '-') and (\d)-(\d) -> $1.$2
    expand(&mut variants, &mut variants_seen, &|variant| {
        vec![variant.replace('.', "-"), digit_dash_to_dot(variant)]
    });
    // trailing date-stamp strip
    expand(&mut variants, &mut variants_seen, &|variant| {
        vec![strip_trailing_date(variant)]
    });
    // provider-prefixed forms
    expand(&mut variants, &mut variants_seen, &|variant| {
        provider_prefixed(variant).into_iter().collect()
    });

    for variant in &variants {
        push(&mut keys, variant.clone());
    }

    // Alias table (MODEL_ALIASES) is empty in the TS source; snapshot-then-push
    // is a no-op here but kept structurally for parity.
    // (no entries)

    keys
}

/// True when every effective rate of `price` is 0 (a placeholder). Faithful
/// port of `isZeroCost`.
pub fn is_zero_cost(price: &ModelPrice) -> bool {
    let flat_zero = price.input == 0.0
        && price.output == 0.0
        && price.cache_read.unwrap_or(0.0) == 0.0
        && price.cache_write.unwrap_or(0.0) == 0.0
        && price.cache_write1h.unwrap_or(0.0) == 0.0;
    if !flat_zero {
        return false;
    }
    for tier in price.tiers.as_deref().unwrap_or(&[]) {
        if tier.input != 0.0
            || tier.output != 0.0
            || tier.cache_read.unwrap_or(0.0) != 0.0
            || tier.cache_write.unwrap_or(0.0) != 0.0
            || tier.cache_write1h.unwrap_or(0.0) != 0.0
        {
            return false;
        }
    }
    true
}

/// The result of `match_model`: either a hit (matched key + price) or a miss
/// (the attempted key list, with an optional zero-cost key that was skipped).
pub enum MatchResult {
    Hit {
        key: String,
        price: ModelPrice,
    },
    Miss {
        attempted: Vec<String>,
        zero_cost_key: Option<String>,
    },
}

/// Match `model` against `prices`. Faithful port of `matchModel`. When
/// `skip_zero_cost` is set, a best-match placeholder (all-zero) short-circuits
/// the whole catalog rather than settling for a lower-confidence key.
pub fn match_model(model: &str, prices: &PriceMap, skip_zero_cost: bool) -> MatchResult {
    let attempted = candidate_keys(model);
    for key in &attempted {
        let Some(price) = prices.get(key) else {
            continue;
        };
        if skip_zero_cost && is_zero_cost(price) {
            let zero_cost_key = Some(key.clone());
            return MatchResult::Miss {
                attempted,
                zero_cost_key,
            };
        }
        return MatchResult::Hit {
            key: key.clone(),
            price: price.clone(),
        };
    }
    MatchResult::Miss {
        attempted,
        zero_cost_key: None,
    }
}
