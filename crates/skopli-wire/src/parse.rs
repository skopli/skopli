//! JSON-boundary parsing shared by the direct-core bindings: reading the
//! `{home, env}` path-override context, parsing `UsageEvent[]`/`TokenCounts`/
//! `ModelPrice` from the camelCase wire shapes the facades send, and the default
//! cache dir. Everything is `serde_json::Value` — no FFI types appear here.

use serde_json::Value;
use skopli_core::readers::reader::ReaderContext;
use skopli_core::rollup::Rollup;
use skopli_core::types::{TokenCounts, UsageEvent};

/// Build a [`ReaderContext`] from the shared path-override options shape:
/// `{ "home": string?, "env": { name: string }?, "cwd": string? }`. Absent
/// `home` falls back to the process home directory; an explicit `env` map is the
/// ENTIRE environment (the process environment is not consulted), matching the
/// TS `PathResolver`. `cwd` drives project-local readers (e.g. trae-agent): an
/// explicit `cwd` in the options wins (test injection), otherwise it defaults to
/// the process working directory; when neither is available the context carries
/// no working directory and project-local readers yield nothing.
pub fn context_from_options(opts: &Value) -> ReaderContext {
    let home = opts
        .get("home")
        .and_then(Value::as_str)
        .map(str::to_owned)
        .unwrap_or_else(home_dir);

    let mut ctx = ReaderContext::new(home);
    if let Some(env) = opts.get("env").and_then(Value::as_object) {
        for (name, value) in env {
            if let Some(v) = value.as_str() {
                ctx = ctx.with_env(name.clone(), v.to_owned());
            }
        }
    }
    let cwd = opts
        .get("cwd")
        .and_then(Value::as_str)
        .filter(|v| !v.is_empty())
        .map(std::path::PathBuf::from)
        .or_else(|| std::env::current_dir().ok());
    if let Some(cwd) = cwd {
        ctx = ctx.with_cwd(cwd);
    }
    ctx
}

/// The process home directory, mirroring node `os.homedir()`. Single source of
/// truth in `skopli_core::paths`.
pub fn home_dir() -> String {
    skopli_core::paths::home_dir()
}

/// Parse a `UsageEvent[]` from a JSON value. `null` is an empty slice.
pub fn parse_events(value: &Value) -> Result<Vec<UsageEvent>, String> {
    if value.is_null() {
        return Ok(Vec::new());
    }
    let arr = value
        .as_array()
        .ok_or_else(|| "events must be a JSON array".to_owned())?;
    let mut out = Vec::with_capacity(arr.len());
    for item in arr {
        out.push(parse_event(item)?);
    }
    Ok(out)
}

/// Parse a `UsageEvent` from a wire object (the same camelCase shape the core
/// serializes out). Absent optionals stay `None`.
fn parse_event(value: &Value) -> Result<UsageEvent, String> {
    let obj = value.as_object().ok_or("each event must be an object")?;
    let get_str = |k: &str| obj.get(k).and_then(Value::as_str);
    let req_str = |k: &str| get_str(k).ok_or_else(|| format!("event needs string \"{k}\""));
    let tokens = parse_tokens(obj.get("tokens").ok_or("event needs \"tokens\"")?)?;
    Ok(UsageEvent {
        harness: req_str("harness")?.to_owned(),
        timestamp: req_str("timestamp")?.to_owned(),
        session_id: req_str("sessionId")?.to_owned(),
        message_id: req_str("messageId")?.to_owned(),
        turn: obj.get("turn").and_then(Value::as_bool).unwrap_or(false),
        subagent: obj
            .get("subagent")
            .and_then(Value::as_bool)
            .unwrap_or(false),
        model: req_str("model")?.to_owned(),
        tokens,
        calls: obj.get("calls").and_then(Value::as_u64),
        cost_usd: obj.get("costUsd").and_then(Value::as_f64),
        workspace: get_str("workspace").map(str::to_owned),
        title: get_str("title").map(str::to_owned),
    })
}

/// Parse a `TokenCounts` from a wire object. Token counters may arrive negative
/// from malformed sources (the TS math is f64-tolerant); clamp to 0 for the u64
/// core counters as the readers do.
pub fn parse_tokens(value: &Value) -> Result<TokenCounts, String> {
    let obj = value.as_object().ok_or("tokens must be an object")?;
    let u = |k: &str| obj.get(k).and_then(Value::as_i64).unwrap_or(0).max(0) as u64;
    Ok(TokenCounts {
        input: u("input"),
        output: u("output"),
        cache_read: u("cacheRead"),
        cache_write: u("cacheWrite"),
        cache_write1h: obj
            .get("cacheWrite1h")
            .and_then(Value::as_i64)
            .map(|v| v.max(0) as u64),
        reasoning: u("reasoning"),
    })
}

/// Parse a `Rollup` from a wire object (the shape `rollup` serializes out:
/// `{key, tokens, events, turns, calls, costUsd?}`). A non-finite `costUsd` is
/// dropped to `None`, mirroring the finite filter `rollup` itself applies, so a
/// parsed rollup priced through the core matches one priced in-process.
pub fn parse_rollup(value: &Value) -> Result<Rollup, String> {
    let obj = value
        .as_object()
        .ok_or_else(|| "each rollup must be an object".to_owned())?;
    let key = obj
        .get("key")
        .and_then(Value::as_str)
        .ok_or_else(|| "a rollup needs a string \"key\"".to_owned())?
        .to_owned();
    let tokens = parse_tokens(obj.get("tokens").ok_or("rollup needs tokens")?)?;
    let u = |k: &str| obj.get(k).and_then(Value::as_u64).unwrap_or(0);
    let cost_usd = obj
        .get("costUsd")
        .and_then(Value::as_f64)
        .filter(|c| c.is_finite());
    Ok(Rollup {
        key,
        tokens,
        events: u("events"),
        turns: u("turns"),
        calls: u("calls"),
        cost_usd,
    })
}

/// Parse a `ModelPrice` for `cost_usd`, accepting the flat wire shape plus an
/// optional `tiers` array and `tierMode` string.
pub fn parse_price(value: &Value) -> Result<skopli_core::pricing::ModelPrice, String> {
    use skopli_core::pricing::types::{PriceTier, TierMode};
    let obj = value.as_object().ok_or("price must be an object")?;
    let num = |k: &str| obj.get(k).and_then(Value::as_f64);
    let input = num("input").ok_or("price needs numeric \"input\"")?;
    let output = num("output").ok_or("price needs numeric \"output\"")?;
    let mut price = skopli_core::pricing::ModelPrice::flat(input, output);
    price.cache_read = num("cacheRead");
    price.cache_write = num("cacheWrite");
    price.cache_write1h = num("cacheWrite1h");
    if let Some(tiers) = obj.get("tiers").and_then(Value::as_array) {
        let mut parsed = Vec::with_capacity(tiers.len());
        for tier in tiers {
            let t = tier.as_object().ok_or("each tier must be an object")?;
            let tnum = |k: &str| t.get(k).and_then(Value::as_f64);
            parsed.push(PriceTier {
                threshold: tnum("threshold").ok_or("tier needs numeric \"threshold\"")?,
                input: tnum("input").ok_or("tier needs numeric \"input\"")?,
                output: tnum("output").ok_or("tier needs numeric \"output\"")?,
                cache_read: tnum("cacheRead"),
                cache_write: tnum("cacheWrite"),
                cache_write1h: tnum("cacheWrite1h"),
            });
        }
        if !parsed.is_empty() {
            price.tiers = Some(parsed);
        }
    }
    price.tier_mode = match obj.get("tierMode").and_then(Value::as_str) {
        Some("marginal") => Some(TierMode::Marginal),
        Some("whole-request") => Some(TierMode::WholeRequest),
        _ => None,
    };
    Ok(price)
}

/// The default on-disk cache directory. Single source of truth in
/// `skopli_core::paths`.
pub fn default_cache_dir() -> String {
    skopli_core::paths::default_cache_dir()
}
