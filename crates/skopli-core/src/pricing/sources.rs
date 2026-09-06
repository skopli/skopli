//! Built-in pricing sources: fetch openrouter/litellm/models.dev, cache to disk,
//! and parse into catalogs for zero-config pricing. Faithful port of the fetch +
//! cache logic in `src/pricing/sources.ts` and `src/pricing/cache.ts`.
//!
//! This is the DATA-LEVEL seam's built-in path: the C
//! ABI has no callback surface, so the built-in sources ARE its seam; callers opt
//! out with `builtin_sources = false` and inject catalog JSON instead. The
//! richer facades (node/py/ruby) keep fetching facade-side in their host language
//! and always pass `builtin_sources = false`.
//!
//! Cache discipline (mirrors cache.ts): a `{fetchedAt, payload}` JSON file per
//! source under the cache dir; ~1h TTL; fresh cache short-circuits the fetch;
//! `offline` is cache-only; a fetch failure falls back to any-age stale cache;
//! writes are atomic (temp file + rename) so a concurrent reader never sees a
//! torn file. `max_cache_age_ms` overrides the TTL.
//!
//! Network I/O uses `ureq` 3 (rustls, blocking) with a 10s timeout. All fetching
//! is confined to [`fetch_json`]; the cache/merge logic is fetch-agnostic and
//! unit-tested via a `file://`-style base-URL indirection with NO network.
//!
//! Without the `net` feature the fetch entrypoint (`load_builtin_catalogs`) is
//! compiled out, so the cache/parse/TTL helpers below are reached only by the
//! unit tests; they stay compiled (network-free) and the `allow(dead_code)`
//! keeps that build warning-free.
#![cfg_attr(not(feature = "net"), allow(dead_code, unused_variables))]

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
#[cfg(feature = "net")]
use std::time::Duration;
use std::time::{SystemTime, UNIX_EPOCH};

use serde_json::Value;

#[cfg(any(feature = "net", test))]
use super::parse::{parse_litellm, parse_models_dev, parse_openrouter};
use super::types::{PriceMap, PricingCatalog};

/// Default cache TTL: one hour (matches the TS `DEFAULT_TTL_MS`).
const DEFAULT_TTL_MS: u64 = 60 * 60 * 1000;

/// Network timeout for a single source fetch.
#[cfg(feature = "net")]
const FETCH_TIMEOUT: Duration = Duration::from_secs(10);

/// Live source URLs (mirror `sources.ts`).
pub const OPENROUTER_URL: &str = "https://openrouter.ai/api/v1/models";
pub const LITELLM_URL: &str =
    "https://raw.githubusercontent.com/BerriAI/litellm/main/model_prices_and_context_window.json";
pub const MODELS_DEV_URL: &str = "https://models.dev/api.json";

/// A parser turning a source's raw JSON body into a `PriceMap`.
type SourceParser = fn(&Value) -> PriceMap;

/// One built-in source spec: `(name, url, parser)`. `name` is both the catalog
/// source label and the cache-file stem (`pricing-<name>.json`).
type SourceSpec<'a> = (&'static str, &'a str, SourceParser);

/// The three built-in sources, in priority order (highest first). `urls`
/// gives the endpoint per source; production uses [`SourceUrls::default`], the
/// net test seam substitutes a local server.
#[cfg(any(feature = "net", test))]
fn builtin_specs(urls: &SourceUrls) -> [SourceSpec<'_>; 3] {
    [
        ("openrouter", &urls.openrouter, parse_openrouter),
        ("litellm", &urls.litellm, parse_litellm),
        ("models-dev", &urls.models_dev, parse_models_dev),
    ]
}

/// Per-source endpoints. Defaults to the live URLs; the net test seam
/// substitutes a local server so the fetch path is exercised without network.
/// Net-gated and hidden from the public API surface: the only public
/// zero-config entry point is [`load_builtin_catalogs`]. This type is `doc(hidden)`
/// so it never appears in the documented core API; it exists only to let the
/// capi crate's net-gated fetch-proving test redirect the built-in fetch at a
/// local server. Also compiled in `test` builds so the hermetic cache/parse unit
/// tests below can construct it without the `net` feature.
#[cfg(any(feature = "net", test))]
#[doc(hidden)]
#[derive(Debug, Clone)]
pub struct SourceUrls {
    pub openrouter: String,
    pub litellm: String,
    pub models_dev: String,
}

#[cfg(any(feature = "net", test))]
impl Default for SourceUrls {
    fn default() -> Self {
        SourceUrls {
            openrouter: OPENROUTER_URL.to_owned(),
            litellm: LITELLM_URL.to_owned(),
            models_dev: MODELS_DEV_URL.to_owned(),
        }
    }
}

/// Options that steer built-in source loading. Mirrors the facade-side
/// `SourceContext` knobs the C ABI carries as `pricing_opts` keys.
#[derive(Debug, Clone)]
pub struct BuiltinOptions {
    /// The on-disk cache directory.
    pub cache_dir: PathBuf,
    /// Cache-only: never touch the network; serve fresh-or-stale cache, else
    /// nothing.
    pub offline: bool,
    /// TTL override in milliseconds (the `max_cache_age_ms` key). `None` uses the
    /// 1h default.
    pub max_cache_age_ms: Option<u64>,
}

impl BuiltinOptions {
    fn ttl_ms(&self) -> u64 {
        self.max_cache_age_ms.unwrap_or(DEFAULT_TTL_MS)
    }
}

/// How to obtain a source payload — the fetch indirection that keeps the cache
/// logic hermetically testable. Production uses [`Fetcher::Network`]; tests use
/// [`Fetcher::Base`] with a local directory as the "server".
enum Fetcher<'a> {
    /// Fetch over HTTP with `ureq`.
    #[cfg(feature = "net")]
    Network,
    /// Read `<base>/<name>.json` from disk instead of the network. Only the
    /// hermetic unit tests construct this variant; production is always
    /// [`Fetcher::Network`].
    #[cfg_attr(not(test), allow(dead_code))]
    Base(&'a Path),
}

impl Fetcher<'_> {
    /// Obtain the raw JSON payload for source `name` from `url`, or an error
    /// string (any error triggers the any-age stale fallback).
    fn get(&self, name: &str, url: &str) -> Result<Value, String> {
        match self {
            #[cfg(feature = "net")]
            Fetcher::Network => fetch_json(url),
            Fetcher::Base(base) => {
                let path = base.join(format!("{name}.json"));
                let raw = std::fs::read_to_string(&path)
                    .map_err(|e| format!("{}: {e}", path.display()))?;
                serde_json::from_str(&raw).map_err(|e| format!("{}: {e}", path.display()))
            }
        }
    }
}

/// Fetch and parse a JSON document over HTTP. Confined to this function so the
/// rest of the module is network-free and unit-testable.
#[cfg(feature = "net")]
fn fetch_json(url: &str) -> Result<Value, String> {
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .timeout_global(Some(FETCH_TIMEOUT))
        .build()
        .into();
    let mut response = agent.get(url).call().map_err(|e| e.to_string())?;
    if response.status() != 200 {
        return Err(format!("{url} responded {}", response.status()));
    }
    response
        .body_mut()
        .read_json::<Value>()
        .map_err(|e| e.to_string())
}

/// A cache payload read back from disk.
struct Cached {
    fetched_at: String,
    payload: Value,
    stale: bool,
}

/// Milliseconds since the Unix epoch, for cache-age comparisons.
fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// The cache file name for a source (`pricing-<name>.json`).
fn cache_name(name: &str) -> String {
    format!("pricing-{name}.json")
}

/// Load and validate the cache file for `name`, computing staleness against
/// `ttl_ms` and `now_ms`. Returns `None` when absent/unparseable/malformed.
/// Faithful port of `loadCached`.
fn load_cached(cache_dir: &Path, name: &str, ttl_ms: u64, now: u64) -> Option<Cached> {
    let path = cache_dir.join(cache_name(name));
    let raw = std::fs::read_to_string(&path).ok()?;
    let parsed: Value = serde_json::from_str(&raw).ok()?;
    let obj = parsed.as_object()?;
    let fetched_at = obj.get("fetchedAt")?.as_str()?.to_owned();
    let payload = obj.get("payload")?.clone();
    // Parse the ISO timestamp back to epoch ms; a bad timestamp invalidates.
    let fetched_ms = parse_iso_ms(&fetched_at)?;
    // Compare in i128 so a TTL above i64::MAX never wraps negative and marks a
    // fresh entry stale; now/fetched_ms fit i64, ttl_ms fits u64, all fit i128.
    let age = i128::from(now) - i128::from(fetched_ms);
    Some(Cached {
        fetched_at,
        payload,
        stale: age > i128::from(ttl_ms),
    })
}

/// Process-wide monotonic counter making each temp-file name unique even when
/// two threads write in the same process within the same millisecond (pid + now
/// alone would collide).
static TEMP_SEQ: AtomicU64 = AtomicU64::new(0);

/// Atomically store `payload` under `pricing-<name>.json` and return the
/// `fetchedAt` ISO timestamp written. Faithful port of `storeCached`: write to a
/// unique temp file then rename, so a concurrent reader never sees a torn file.
fn store_cached(cache_dir: &Path, name: &str, payload: &Value, now: u64) -> Result<String, String> {
    let fetched_at = iso_from_ms(now);
    let file = serde_json::json!({ "fetchedAt": fetched_at, "payload": payload });
    let serialized = serde_json::to_string(&file).map_err(|e| e.to_string())?;
    std::fs::create_dir_all(cache_dir).map_err(|e| e.to_string())?;
    let seq = TEMP_SEQ.fetch_add(1, Ordering::Relaxed);
    let unique = format!("{}.{}.{}.tmp", std::process::id(), now, seq);
    let tmp = cache_dir.join(format!("{}.{unique}", cache_name(name)));
    std::fs::write(&tmp, serialized).map_err(|e| e.to_string())?;
    let dest = cache_dir.join(cache_name(name));
    match std::fs::rename(&tmp, &dest) {
        Ok(()) => Ok(fetched_at),
        Err(e) => {
            let _ = std::fs::remove_file(&tmp);
            Err(e.to_string())
        }
    }
}

/// Load all built-in catalogs in priority order, honouring the cache + offline
/// options. Production entry point (network-backed, live URLs).
#[cfg(feature = "net")]
pub fn load_builtin_catalogs(opts: &BuiltinOptions, refresh: bool) -> Vec<PricingCatalog> {
    load_builtin_catalogs_with_urls(opts, refresh, &SourceUrls::default())
}

/// As [`load_builtin_catalogs`] but with caller-supplied source URLs, so a test
/// can point the network fetch at a local server. Production always calls the
/// wrapper above with the live defaults; this `doc(hidden)` seam keeps the fetch
/// path exercised without leaking test-only surface into the zero-config entry
/// point or the documented public core API.
#[cfg(feature = "net")]
#[doc(hidden)]
pub fn load_builtin_catalogs_with_urls(
    opts: &BuiltinOptions,
    refresh: bool,
    urls: &SourceUrls,
) -> Vec<PricingCatalog> {
    load_with(opts, refresh, &Fetcher::Network, urls)
}

/// Shared loader over an injectable [`Fetcher`]. Iterates the built-in specs,
/// applying the cache/offline/stale-fallback discipline per source.
#[cfg(any(feature = "net", test))]
fn load_with(
    opts: &BuiltinOptions,
    refresh: bool,
    fetcher: &Fetcher,
    urls: &SourceUrls,
) -> Vec<PricingCatalog> {
    let mut catalogs = Vec::new();
    let ttl = opts.ttl_ms();
    let now = now_ms();
    for (name, url, parse) in builtin_specs(urls) {
        if let Some(catalog) = load_one(opts, refresh, fetcher, name, url, parse, ttl, now) {
            catalogs.push(catalog);
        }
    }
    catalogs
}

/// Resolve a single source: fresh cache short-circuits; offline is cache-only;
/// otherwise fetch, and on failure fall back to any-age stale cache. Faithful
/// port of `cachedSource.load`.
#[allow(clippy::too_many_arguments)]
fn load_one(
    opts: &BuiltinOptions,
    refresh: bool,
    fetcher: &Fetcher,
    name: &str,
    url: &str,
    parse: fn(&Value) -> PriceMap,
    ttl: u64,
    now: u64,
) -> Option<PricingCatalog> {
    let cached = load_cached(&opts.cache_dir, name, ttl, now);
    // A cache payload that parses to zero prices is unusable, same as no cache.
    let cached_catalog = cached.as_ref().and_then(|c| {
        let prices = parse(&c.payload);
        if prices.is_empty() {
            None
        } else {
            Some(PricingCatalog {
                source: name.to_owned(),
                fetched_at: Some(c.fetched_at.clone()),
                prices,
            })
        }
    });

    let cache_fresh = cached.as_ref().is_some_and(|c| !c.stale) && !refresh;
    if cached_catalog.is_some() && (cache_fresh || opts.offline) {
        return cached_catalog;
    }
    if opts.offline {
        return None;
    }

    let payload = match fetcher.get(name, url) {
        Ok(payload) => payload,
        // Any-age stale fallback keeps pricing available when the network is not.
        Err(_) => return cached_catalog,
    };
    let prices = parse(&payload);
    if prices.is_empty() {
        return cached_catalog;
    }
    let fetched_at =
        store_cached(&opts.cache_dir, name, &payload, now).unwrap_or_else(|_| iso_from_ms(now));
    Some(PricingCatalog {
        source: name.to_owned(),
        fetched_at: Some(fetched_at),
        prices,
    })
}

/// Format epoch-ms as an ISO-8601 UTC timestamp (`YYYY-MM-DDTHH:MM:SS.mmmZ`),
/// matching JS `new Date(ms).toISOString()`.
fn iso_from_ms(ms: u64) -> String {
    let secs = (ms / 1000) as i64;
    let millis = (ms % 1000) as u32;
    let (year, month, day, hour, min, sec) = civil_from_epoch_secs(secs);
    format!("{year:04}-{month:02}-{day:02}T{hour:02}:{min:02}:{sec:02}.{millis:03}Z")
}

/// Parse an ISO-8601 timestamp back to epoch-ms, matching JS `Date.parse` on the
/// stamps a cache file can carry: `YYYY-MM-DDTHH:MM:SS` with an optional
/// fractional-second part (truncated to integral ms) and either a `Z` suffix or
/// a `+HH:MM` / `-HH:MM` UTC offset. Pre-epoch stamps yield a negative result;
/// anything malformed is `None` (invalidating the cache entry, like a `NaN`
/// `Date.parse`).
fn parse_iso_ms(s: &str) -> Option<i64> {
    let bytes = s.as_bytes();
    if bytes.len() < 19 || bytes[4] != b'-' || bytes[7] != b'-' || bytes[10] != b'T' {
        return None;
    }
    let year: i64 = s.get(0..4)?.parse().ok()?;
    let month: u32 = s.get(5..7)?.parse().ok()?;
    let day: u32 = s.get(8..10)?.parse().ok()?;
    let hour: u32 = s.get(11..13)?.parse().ok()?;
    let min: u32 = s.get(14..16)?.parse().ok()?;
    let sec: u32 = s.get(17..19)?.parse().ok()?;
    let mut rest = &s[19..];

    let mut millis: i64 = 0;
    if rest.starts_with('.') {
        let frac_end = rest[1..]
            .find(|c: char| !c.is_ascii_digit())
            .map(|i| i + 1)
            .unwrap_or(rest.len());
        let frac = &rest[1..frac_end];
        if frac.is_empty() {
            return None;
        }
        let mut digits = frac.bytes().chain(std::iter::repeat(b'0'));
        for _ in 0..3 {
            millis = millis * 10 + (digits.next().unwrap() - b'0') as i64;
        }
        rest = &rest[frac_end..];
    }

    let offset_secs: i64 = match rest.as_bytes().first() {
        Some(b'Z') if rest.len() == 1 => 0,
        Some(b'+') | Some(b'-') if rest.len() == 6 && rest.as_bytes()[3] == b':' => {
            let sign = if rest.starts_with('-') { -1 } else { 1 };
            let oh: i64 = rest.get(1..3)?.parse().ok()?;
            let om: i64 = rest.get(4..6)?.parse().ok()?;
            sign * (oh * 3600 + om * 60)
        }
        _ => return None,
    };

    let days = epoch_days_from_civil(year, month, day)?;
    let total_secs =
        days * 86400 + (hour as i64) * 3600 + (min as i64) * 60 + sec as i64 - offset_secs;
    Some(total_secs * 1000 + millis)
}

/// Days since the Unix epoch for a civil (proleptic Gregorian) date.
/// Howard Hinnant's `days_from_civil` algorithm.
fn epoch_days_from_civil(year: i64, month: u32, day: u32) -> Option<i64> {
    if !(1..=12).contains(&month) || !(1..=31).contains(&day) {
        return None;
    }
    let y = if month <= 2 { year - 1 } else { year };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let m = month as i64;
    let d = day as i64;
    let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    Some(era * 146097 + doe - 719468)
}

/// Inverse of [`epoch_days_from_civil`] plus the time-of-day, from epoch secs.
/// Howard Hinnant's `civil_from_days` algorithm.
fn civil_from_epoch_secs(secs: i64) -> (i64, u32, u32, u32, u32, u32) {
    let days = secs.div_euclid(86400);
    let rem = secs.rem_euclid(86400);
    let hour = (rem / 3600) as u32;
    let min = ((rem % 3600) / 60) as u32;
    let sec = (rem % 60) as u32;
    let z = days + 719468;
    let era = if z >= 0 { z } else { z - 146096 } / 146097;
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = (if mp < 10 { mp + 3 } else { mp - 9 }) as u32;
    let year = if m <= 2 { y + 1 } else { y };
    (year, m, d, hour, min, sec)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp_dir(tag: &str) -> PathBuf {
        let base = std::env::temp_dir().join(format!(
            "skopli-sources-test-{tag}-{}-{}",
            std::process::id(),
            now_ms()
        ));
        std::fs::create_dir_all(&base).unwrap();
        base
    }

    fn openrouter_payload() -> Value {
        serde_json::json!({
            "data": [
                { "id": "vendor/model-a", "pricing": { "prompt": "0.000001", "completion": "0.000002" } }
            ]
        })
    }

    #[test]
    fn iso_round_trips_epoch_ms() {
        // 2026-08-01T09:00:00.000Z
        let ms = parse_iso_ms("2026-08-01T09:00:00.000Z").unwrap();
        assert_eq!(iso_from_ms(ms as u64), "2026-08-01T09:00:00.000Z");
        // A known epoch: 2001-09-09T01:46:40.000Z == 1_000_000_000_000 ms.
        assert_eq!(iso_from_ms(1_000_000_000_000), "2001-09-09T01:46:40.000Z");
        assert_eq!(
            parse_iso_ms("2001-09-09T01:46:40.000Z"),
            Some(1_000_000_000_000)
        );
    }

    #[test]
    fn timestamp_boundary_conformance() {
        // Driven by the SHARED fixtures in golden/pricing/timestamps/. The cache
        // fetchedAt stamp is one wire contract read and written by seven facades;
        // every case is consumed through the PRODUCTION cache path (load_cached /
        // store_cached) so the core agrees with the shared gold on every boundary.
        #[derive(serde::Deserialize)]
        struct ReadCase {
            name: String,
            stamp: String,
            #[serde(rename = "epochMs")]
            epoch_ms: Option<i64>,
        }
        #[derive(serde::Deserialize)]
        struct Cases {
            read: Vec<ReadCase>,
            write: Vec<u64>,
        }

        let path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .and_then(Path::parent)
            .expect("repo root")
            .join("golden")
            .join("pricing")
            .join("timestamps")
            .join("cases.json");
        let cases: Cases =
            serde_json::from_str(&std::fs::read_to_string(&path).expect("read cases.json"))
                .expect("parse cases.json");
        assert!(!cases.read.is_empty(), "no read cases found");

        let name = "openrouter";

        // Read cases: write a cache document carrying the fixture stamp, then
        // consume it through load_cached. Garbage must invalidate the entry
        // (None); a valid epoch must sit exactly on the fresh/stale boundary.
        for case in &cases.read {
            let dir = tmp_dir(&format!("ts-read-{}", case.name));
            let file = serde_json::json!({
                "fetchedAt": case.stamp,
                "payload": openrouter_payload(),
            });
            std::fs::write(
                dir.join(cache_name(name)),
                serde_json::to_string(&file).unwrap(),
            )
            .unwrap();
            match case.epoch_ms {
                None => assert!(
                    load_cached(&dir, name, 0, 0).is_none(),
                    "read {}: garbage must invalidate",
                    case.name
                ),
                Some(ms) => {
                    // Pin the parsed epoch through the real age math with a fixed
                    // positive `now` (now_ms is unsigned) and a TTL set to the exact
                    // age: age == ttl is fresh, age == ttl-1 (a 1ms shorter TTL) is
                    // stale. This brackets the parsed instant for negative epochs
                    // too, where a positive `now` cannot equal the stamp.
                    let now: u64 = 2_000_000_000_000;
                    let age = i128::from(now) - i128::from(ms);
                    assert!(age >= 0, "read {}: chosen now precedes stamp", case.name);
                    let ttl = age as u64;
                    let fresh = load_cached(&dir, name, ttl, now)
                        .unwrap_or_else(|| panic!("read {}: fresh load", case.name));
                    assert!(!fresh.stale, "read {}: age==ttl must be fresh", case.name);
                    let stale = load_cached(&dir, name, ttl - 1, now)
                        .unwrap_or_else(|| panic!("read {}: stale load", case.name));
                    assert!(stale.stale, "read {}: age==ttl+1 must be stale", case.name);
                }
            }
            std::fs::remove_dir_all(&dir).ok();
        }

        // Write cases: store through store_cached, read the persisted document,
        // assert the real fetchedAt shape, then load it back through load_cached.
        for &ms in &cases.write {
            let dir = tmp_dir(&format!("ts-write-{ms}"));
            let stamp = store_cached(&dir, name, &openrouter_payload(), ms).unwrap();
            assert_eq!(stamp.len(), 24, "write {ms}: length");
            assert!(stamp.ends_with('Z'), "write {ms}: Z suffix");
            let raw = std::fs::read_to_string(dir.join(cache_name(name))).unwrap();
            let doc: Value = serde_json::from_str(&raw).unwrap();
            assert_eq!(
                doc.get("fetchedAt").and_then(Value::as_str),
                Some(stamp.as_str()),
                "write {ms}: persisted fetchedAt"
            );
            let fresh = load_cached(&dir, name, 0, ms).unwrap();
            assert!(
                !fresh.stale,
                "write {ms}: reload at now==epoch must be fresh"
            );
            let stale = load_cached(&dir, name, 0, ms + 1).unwrap();
            assert!(
                stale.stale,
                "write {ms}: reload at now==epoch+1 must be stale"
            );
            std::fs::remove_dir_all(&dir).ok();
        }
    }

    #[test]
    fn max_u64_ttl_keeps_fresh_stamp_fresh() {
        // Regression: a max_cache_age_ms of u64::MAX must never wrap the age
        // comparison negative and mark a fresh cache stale. With the old
        // `ttl_ms as i64` cast, u64::MAX became -1 and age 0 satisfied 0 > -1.
        let dir = tmp_dir("ttl-u64-max");
        let now = 1_000_000_000_000;
        store_cached(&dir, "openrouter", &openrouter_payload(), now).unwrap();
        let cached = load_cached(&dir, "openrouter", u64::MAX, now).unwrap();
        assert!(
            !cached.stale,
            "u64::MAX TTL with a fresh stamp must be fresh"
        );
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn store_then_load_is_fresh_and_atomic() {
        let dir = tmp_dir("atomic");
        let now = 1_000_000_000_000;
        let at = store_cached(&dir, "openrouter", &openrouter_payload(), now).unwrap();
        assert_eq!(at, "2001-09-09T01:46:40.000Z");
        // No stray temp files remain after the rename.
        let leftovers: Vec<_> = std::fs::read_dir(&dir)
            .unwrap()
            .filter_map(Result::ok)
            .filter(|e| e.file_name().to_string_lossy().ends_with(".tmp"))
            .collect();
        assert!(leftovers.is_empty(), "temp files leaked: {leftovers:?}");
        // Within TTL → fresh.
        let cached = load_cached(&dir, "openrouter", DEFAULT_TTL_MS, now + 1000).unwrap();
        assert!(!cached.stale);
        // Past TTL → stale but still loadable.
        let cached =
            load_cached(&dir, "openrouter", DEFAULT_TTL_MS, now + DEFAULT_TTL_MS + 1).unwrap();
        assert!(cached.stale);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn fresh_cache_short_circuits_fetch() {
        let dir = tmp_dir("fresh");
        let now = now_ms();
        store_cached(&dir, "openrouter", &openrouter_payload(), now).unwrap();
        let opts = BuiltinOptions {
            cache_dir: dir.clone(),
            offline: false,
            max_cache_age_ms: None,
        };
        // Empty base dir → any fetch would fail; a fresh cache must avoid it.
        let empty_base = tmp_dir("fresh-base");
        let catalogs = load_with(
            &opts,
            false,
            &Fetcher::Base(&empty_base),
            &SourceUrls::default(),
        );
        assert_eq!(catalogs.len(), 1);
        assert_eq!(catalogs[0].source, "openrouter");
        std::fs::remove_dir_all(&dir).ok();
        std::fs::remove_dir_all(&empty_base).ok();
    }

    #[test]
    fn stale_cache_fetches_and_refreshes() {
        let dir = tmp_dir("stale");
        // Write a cache far in the past so it is stale.
        store_cached(&dir, "openrouter", &openrouter_payload(), 1_000_000_000_000).unwrap();
        let base = tmp_dir("stale-base");
        std::fs::write(
            base.join("openrouter.json"),
            serde_json::to_string(&openrouter_payload()).unwrap(),
        )
        .unwrap();
        let opts = BuiltinOptions {
            cache_dir: dir.clone(),
            offline: false,
            max_cache_age_ms: None,
        };
        let catalogs = load_with(&opts, false, &Fetcher::Base(&base), &SourceUrls::default());
        assert_eq!(catalogs.len(), 1);
        // The refreshed cache is now fresh (fetchedAt updated to ~now).
        let reloaded = load_cached(&dir, "openrouter", DEFAULT_TTL_MS, now_ms()).unwrap();
        assert!(!reloaded.stale);
        std::fs::remove_dir_all(&dir).ok();
        std::fs::remove_dir_all(&base).ok();
    }

    #[test]
    fn fetch_failure_falls_back_to_any_age_stale() {
        let dir = tmp_dir("fallback");
        store_cached(&dir, "openrouter", &openrouter_payload(), 1_000_000_000_000).unwrap();
        // Empty base → fetch fails; stale cache must still be served.
        let empty_base = tmp_dir("fallback-base");
        let opts = BuiltinOptions {
            cache_dir: dir.clone(),
            offline: false,
            max_cache_age_ms: None,
        };
        let catalogs = load_with(
            &opts,
            false,
            &Fetcher::Base(&empty_base),
            &SourceUrls::default(),
        );
        assert_eq!(catalogs.len(), 1);
        assert_eq!(catalogs[0].source, "openrouter");
        std::fs::remove_dir_all(&dir).ok();
        std::fs::remove_dir_all(&empty_base).ok();
    }

    #[test]
    fn offline_is_cache_only() {
        let dir = tmp_dir("offline");
        store_cached(&dir, "openrouter", &openrouter_payload(), 1_000_000_000_000).unwrap();
        // Base has data but offline must never consult it.
        let base = tmp_dir("offline-base");
        std::fs::write(
            base.join("litellm.json"),
            serde_json::to_string(&serde_json::json!({
                "some/model": { "input_cost_per_token": 0.000001, "output_cost_per_token": 0.000002 }
            }))
            .unwrap(),
        )
        .unwrap();
        let opts = BuiltinOptions {
            cache_dir: dir.clone(),
            offline: true,
            max_cache_age_ms: None,
        };
        let catalogs = load_with(&opts, false, &Fetcher::Base(&base), &SourceUrls::default());
        // Only the cached openrouter source; litellm (base-only) is NOT fetched.
        assert_eq!(catalogs.len(), 1);
        assert_eq!(catalogs[0].source, "openrouter");
        std::fs::remove_dir_all(&dir).ok();
        std::fs::remove_dir_all(&base).ok();
    }

    #[test]
    fn max_cache_age_override_forces_stale() {
        let dir = tmp_dir("ttl");
        let now = now_ms();
        store_cached(&dir, "openrouter", &openrouter_payload(), now).unwrap();
        // TTL of 0 ms makes even a just-written cache stale → it must fetch.
        let base = tmp_dir("ttl-base");
        std::fs::write(
            base.join("openrouter.json"),
            serde_json::to_string(&openrouter_payload()).unwrap(),
        )
        .unwrap();
        let opts = BuiltinOptions {
            cache_dir: dir.clone(),
            offline: false,
            max_cache_age_ms: Some(0),
        };
        // With an empty base it would still fall back to stale; here the base has
        // data so a fresh fetch happens. Either way we get a catalog; assert the
        // TTL classification directly.
        let cached = load_cached(&dir, "openrouter", 0, now + 1).unwrap();
        assert!(cached.stale, "0ms TTL must classify as stale");
        let catalogs = load_with(&opts, false, &Fetcher::Base(&base), &SourceUrls::default());
        assert_eq!(catalogs.len(), 1);
        std::fs::remove_dir_all(&dir).ok();
        std::fs::remove_dir_all(&base).ok();
    }
}
