//! Aggregation of [`UsageEvent`]s into keyed [`Rollup`]s. Faithful port of
//! `src/rollup.ts`.
//!
//! Rollups group events by one dimension (`model`, `day`, `session`, `harness`,
//! or `workspace`), summing token counters, event/turn/call counts, and any
//! recorded `costUsd`. The returned list is sorted ascending by `key`
//! (byte-wise, matching the TS `a.key < b.key` string compare), so the output
//! order is deterministic. Day rollups bucket on the calendar date in the given
//! IANA timezone; the conformance suite always passes `tz: "UTC"` (hermetic).
//!
//! Day rollups derive the calendar date of each event's timestamp in the
//! requested IANA zone (via a bundled tzdb, so results are identical on every
//! platform including Windows, which ships no system tzdb). DST transitions are
//! honored. An unknown or unparseable zone falls back to UTC, matching the way
//! the TS reference degrades an unusable zone rather than aborting a rollup.
//!
//! Token counters are `u64` in Rust (by contract, counters stay integers).
//! `cacheWrite1h` is clamped per event into `[0, cacheWrite]` before
//! summing so one malformed event cannot rebill other events' writes at the 1h
//! rate; `calls`/`costUsd` treat non-finite reported values as absent.

use serde::Serialize;

use crate::types::{TokenCounts, UsageEvent};

/// The dimension a [`rollup`] groups by. Mirrors `RollupBy` in src/rollup.ts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RollupBy {
    Model,
    Day,
    Session,
    Harness,
    Workspace,
    Block,
}

impl RollupBy {
    /// Parse the wire name (as it appears in `input-manifest.json`), or `None`.
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "model" => Some(Self::Model),
            "day" => Some(Self::Day),
            "session" => Some(Self::Session),
            "harness" => Some(Self::Harness),
            "workspace" => Some(Self::Workspace),
            "block" => Some(Self::Block),
            _ => None,
        }
    }
}

/// The default billing-block width: five hours in milliseconds.
pub const DEFAULT_BLOCK_MS: i64 = 18_000_000;

/// Options controlling a [`rollup`]. Mirrors `RollupOptions` in src/rollup.ts.
#[derive(Debug, Clone)]
pub struct RollupOptions {
    pub by: RollupBy,
    /// IANA timezone for day bucketing and block-start hour flooring. An omitted
    /// `tz` (`None`) resolves the system timezone (matching the TS default; jiff
    /// falls back to UTC if the system zone is undetectable). `"UTC"` buckets on
    /// the UTC calendar date (the hermetic default the conformance suite pins,
    /// which the exporter always passes). An unknown zone falls back to UTC.
    pub tz: Option<String>,
    /// Billing-block width in milliseconds (`by == Block` only). `None` uses
    /// [`DEFAULT_BLOCK_MS`]; a non-positive value also falls back to it.
    pub block_ms: Option<i64>,
}

impl RollupOptions {
    /// A rollup by `by` with system-timezone day bucketing (the `tz`-omitted
    /// default) and the default block width.
    pub fn new(by: RollupBy) -> Self {
        Self {
            by,
            tz: None,
            block_ms: None,
        }
    }
}

/// One aggregated bucket. Mirrors `Rollup` in src/rollup.ts; field order matches
/// the gold-file grammar (the comparator ignores key order regardless). Optional
/// `cost_usd` is OMITTED when absent (never serialized as `null`).
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Rollup {
    pub key: String,
    pub tokens: TokenCounts,
    pub events: u64,
    pub turns: u64,
    /// Underlying model calls; may exceed `events` for session-aggregate
    /// sources (each event contributes its reported finite count, else 1).
    pub calls: u64,
    #[serde(rename = "costUsd", skip_serializing_if = "Option::is_none")]
    pub cost_usd: Option<f64>,
}

fn empty_tokens() -> TokenCounts {
    TokenCounts {
        input: 0,
        output: 0,
        cache_read: 0,
        cache_write: 0,
        cache_write1h: None,
        reasoning: 0,
    }
}

/// The `YYYY-MM-DD` calendar date for `timestamp` in `tz`. Mirrors `dayKey` in
/// src/rollup.ts. An omitted `tz` resolves the system IANA zone (matching the TS
/// default of the process system timezone; jiff falls back to UTC if the system
/// zone is undetectable); `"UTC"` gives the UTC calendar date; any other IANA
/// zone gives the local calendar date via the bundled tzdb (DST-aware). An
/// unknown zone falls back to UTC. An unparseable timestamp yields
/// `"invalid-date"`, matching the TS `Number.isNaN` guard.
fn day_key(timestamp: &str, tz: Option<&str>) -> String {
    let Some(ms) = crate::readers::shared::date_parse_ms(timestamp) else {
        return "invalid-date".to_owned();
    };

    // `"UTC"` stays on the byte-exact `to_iso_string` path so the hermetic UTC
    // gold is untouched; an omitted zone resolves the system zone and any named
    // zone routes through the tzdb.
    match tz {
        Some("UTC") => utc_day_key(ms),
        None => local_day_key(ms, &jiff::tz::TimeZone::system()),
        Some(name) => match jiff::tz::TimeZone::get(name) {
            Ok(zone) => local_day_key(ms, &zone),
            // Unknown zone degrades to UTC rather than aborting the rollup.
            Err(_) => utc_day_key(ms),
        },
    }
}

/// The UTC `YYYY-MM-DD` for `ms`, via the V8-faithful ISO formatter.
fn utc_day_key(ms: i64) -> String {
    match crate::time::to_iso_string(ms) {
        // toISOString grammar is `YYYY-MM-DDT...`; the date is the first 10
        // chars for years in `0000..=9999` (every timestamp the readers emit).
        // Expanded-year forms fall back to the whole prefix before `T`, still a
        // stable key.
        Some(iso) => iso.split('T').next().unwrap_or(&iso).to_owned(),
        None => "invalid-date".to_owned(),
    }
}

/// The `YYYY-MM-DD` local calendar date of `ms` in `zone`. An out-of-range
/// millisecond count (beyond jiff's timestamp domain) falls back to the UTC key.
fn local_day_key(ms: i64, zone: &jiff::tz::TimeZone) -> String {
    match jiff::Timestamp::from_millisecond(ms) {
        Ok(ts) => {
            let date = zone.to_datetime(ts).date();
            format!("{:04}-{:02}-{:02}", date.year(), date.month(), date.day())
        }
        Err(_) => utc_day_key(ms),
    }
}

/// The rollup key for `event` under `options`. Mirrors `rollupKey`. Not used
/// for [`RollupBy::Block`], which keys on the block-start instant rather than a
/// per-event field (see [`block_rollup`]).
pub fn rollup_key(event: &UsageEvent, options: &RollupOptions) -> String {
    match options.by {
        RollupBy::Model => event.model.clone(),
        RollupBy::Day => day_key(&event.timestamp, options.tz.as_deref()),
        RollupBy::Session => event.session_id.clone(),
        RollupBy::Harness => event.harness.clone(),
        RollupBy::Workspace => event
            .workspace
            .clone()
            .unwrap_or_else(|| "(unknown)".to_owned()),
        // A block's key is derived from event ordering, not a single event; the
        // grouping loop never calls this for Block.
        RollupBy::Block => day_key(&event.timestamp, options.tz.as_deref()),
    }
}

/// Accumulate one `event`'s counters into `entry`. Shared by the keyed and
/// block rollup paths so both bill tokens/calls/cost identically.
fn accumulate(entry: &mut Rollup, event: &UsageEvent) {
    entry.tokens.input += event.tokens.input;
    entry.tokens.output += event.tokens.output;
    entry.tokens.cache_read += event.tokens.cache_read;
    entry.tokens.cache_write += event.tokens.cache_write;
    if let Some(reported1h) = event.tokens.cache_write1h {
        // Clamp each event's 1h split into [0, its own cacheWrite] before
        // summing (u64 counters are already >= 0, so the clamp is just the
        // upper bound against this event's cacheWrite).
        let clamped1h = reported1h.min(event.tokens.cache_write);
        entry.tokens.cache_write1h = Some(entry.tokens.cache_write1h.unwrap_or(0) + clamped1h);
    }
    entry.tokens.reasoning += event.tokens.reasoning;
    entry.events += 1;
    if event.turn {
        entry.turns += 1;
    }
    // An event with no reported call count still represents at least one
    // call; a reported count is a finite u64 (readers already reject
    // NaN/Infinity), else it is absent and counts as one.
    entry.calls += event.calls.unwrap_or(1);
    if let Some(recorded) = event.cost_usd
        && recorded.is_finite()
    {
        entry.cost_usd = Some(entry.cost_usd.unwrap_or(0.0) + recorded);
    }
}

fn new_bucket(key: String) -> Rollup {
    Rollup {
        key,
        tokens: empty_tokens(),
        events: 0,
        turns: 0,
        calls: 0,
        cost_usd: None,
    }
}

/// The epoch millisecond of `ms` floored to the start of its clock hour in
/// `tz`. Explicit `"UTC"` floors via integer arithmetic; an omitted `tz`
/// floors in the system zone; a named zone floors the local wall-clock hour
/// (honoring fractional-hour offsets) and converts back to the instant. An
/// unparseable millisecond or unknown zone falls back to the UTC-hour floor.
/// Mirrors `floorToHourMs` in src/rollup.ts.
fn floor_to_hour_ms(ms: i64, tz: Option<&str>) -> i64 {
    match tz {
        Some("UTC") => floor_utc_hour(ms),
        None => floor_local_hour(ms, &jiff::tz::TimeZone::system()),
        Some(name) => match jiff::tz::TimeZone::get(name) {
            Ok(zone) => floor_local_hour(ms, &zone),
            Err(_) => floor_utc_hour(ms),
        },
    }
}

fn floor_utc_hour(ms: i64) -> i64 {
    const HOUR_MS: i64 = 3_600_000;
    ms - ms.rem_euclid(HOUR_MS)
}

/// Floor `ms` to the start of its wall-clock hour in `zone` and return the
/// instant. The local civil time gives the event's minute/second/subsecond past
/// the hour; subtracting that duration from the original instant reaches the
/// hour start while preserving which occurrence of a repeated (fall-back) hour
/// the event belongs to. Re-resolving the floored civil time through the zone
/// would instead collapse both occurrences to the earlier one, so this operates
/// on the instant directly, matching the TS reference. An out-of-range instant
/// falls back to the UTC-hour floor.
fn floor_local_hour(ms: i64, zone: &jiff::tz::TimeZone) -> i64 {
    let Ok(ts) = jiff::Timestamp::from_millisecond(ms) else {
        return floor_utc_hour(ms);
    };
    let dt = zone.to_datetime(ts);
    let sub_second = ms.rem_euclid(1000);
    let within_hour = i64::from(dt.minute()) * 60_000 + i64::from(dt.second()) * 1000 + sub_second;
    ms - within_hour
}

/// Aggregate `events` into session-window billing blocks. Events are ordered by
/// timestamp; a block opens at the first event's timestamp floored to the hour
/// in `tz` and spans `block_ms`. Events whose timestamp lies in
/// `[start, start + block_ms)` join the block; the first event past that opens
/// a new block. The row key is the ISO instant of the block start. Mirrors
/// `blockRollup` in src/rollup.ts.
fn block_rollup(events: &[UsageEvent], options: &RollupOptions) -> Vec<Rollup> {
    let block_ms = match options.block_ms {
        Some(v) if v > 0 => v,
        _ => DEFAULT_BLOCK_MS,
    };
    let tz = options.tz.as_deref();

    // Order by parsed timestamp; events with an unparseable timestamp sort last
    // and share a single "invalid-date" block so a bad row never derails the
    // valid windows. A stable sort preserves input order within a tie.
    let mut ordered: Vec<(&UsageEvent, Option<i64>)> = events
        .iter()
        .map(|e| (e, crate::readers::shared::date_parse_ms(&e.timestamp)))
        .collect();
    ordered.sort_by(|a, b| match (a.1, b.1) {
        (Some(x), Some(y)) => x.cmp(&y),
        (Some(_), None) => std::cmp::Ordering::Less,
        (None, Some(_)) => std::cmp::Ordering::Greater,
        (None, None) => std::cmp::Ordering::Equal,
    });

    let mut out: Vec<Rollup> = Vec::new();
    let mut block_start_ms: Option<i64> = None;
    for (event, ms) in ordered {
        let Some(ms) = ms else {
            // Route unparseable timestamps to one shared bucket; never mixes
            // with a live window because they sort to the end.
            if out.last().map(|r| r.key.as_str()) != Some("invalid-date") {
                out.push(new_bucket("invalid-date".to_owned()));
            }
            accumulate(out.last_mut().expect("bucket present"), event);
            continue;
        };
        // Saturating add so an extreme width can never panic (debug) or wrap
        // (release): a saturated end never splits, matching the TS float add,
        // which grows without wrapping at extreme widths.
        let in_open_block = block_start_ms.is_some_and(|start| ms < start.saturating_add(block_ms));
        if !in_open_block {
            let start = floor_to_hour_ms(ms, tz);
            block_start_ms = Some(start);
            let key =
                crate::time::to_iso_string(start).unwrap_or_else(|| "invalid-date".to_owned());
            out.push(new_bucket(key));
        }
        accumulate(out.last_mut().expect("bucket present"), event);
    }
    out
}

/// Aggregate `events` into keyed [`Rollup`]s, sorted ascending by `key`.
/// Faithful port of `rollup` in src/rollup.ts.
pub fn rollup(events: &[UsageEvent], options: &RollupOptions) -> Vec<Rollup> {
    if options.by == RollupBy::Block {
        return block_rollup(events, options);
    }

    // Insertion-order map so the first-seen key order is stable before the final
    // sort; the sort makes order fully deterministic regardless.
    let mut order: Vec<String> = Vec::new();
    let mut map: std::collections::HashMap<String, Rollup> = std::collections::HashMap::new();

    for event in events {
        let key = rollup_key(event, options);
        let entry = map.entry(key.clone()).or_insert_with(|| {
            order.push(key.clone());
            new_bucket(key.clone())
        });
        accumulate(entry, event);
    }

    let mut out: Vec<Rollup> = order
        .into_iter()
        .map(|k| map.remove(&k).expect("key present"))
        .collect();
    out.sort_by(|a, b| a.key.cmp(&b.key));
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn event(model: &str, ts: &str) -> UsageEvent {
        UsageEvent {
            harness: "h".to_owned(),
            timestamp: ts.to_owned(),
            session_id: "s".to_owned(),
            message_id: "m".to_owned(),
            turn: false,
            subagent: false,
            model: model.to_owned(),
            tokens: TokenCounts {
                input: 1,
                output: 2,
                cache_read: 3,
                cache_write: 10,
                cache_write1h: None,
                reasoning: 4,
            },
            calls: None,
            cost_usd: None,
            workspace: None,
            title: None,
        }
    }

    #[test]
    fn groups_by_model_sorted() {
        let events = vec![
            event("zeta", "2026-01-01T00:00:00.000Z"),
            event("alpha", "2026-01-01T00:00:00.000Z"),
            event("alpha", "2026-01-01T00:00:00.000Z"),
        ];
        let out = rollup(&events, &RollupOptions::new(RollupBy::Model));
        assert_eq!(out.len(), 2);
        assert_eq!(out[0].key, "alpha");
        assert_eq!(out[0].events, 2);
        assert_eq!(out[0].tokens.input, 2);
        assert_eq!(out[1].key, "zeta");
        assert_eq!(out[1].events, 1);
    }

    #[test]
    fn day_bucketing_is_utc() {
        // 23:30Z on Jan 1 and 00:30Z on Jan 2 land in different UTC days.
        let events = vec![
            event("m", "2026-01-01T23:30:00.000Z"),
            event("m", "2026-01-02T00:30:00.000Z"),
        ];
        let out = rollup(
            &events,
            &RollupOptions {
                by: RollupBy::Day,
                tz: Some("UTC".to_owned()),
                block_ms: None,
            },
        );
        assert_eq!(out.len(), 2);
        assert_eq!(out[0].key, "2026-01-01");
        assert_eq!(out[1].key, "2026-01-02");
    }

    fn day_rollup(tz: &str, events: Vec<UsageEvent>) -> Vec<Rollup> {
        rollup(
            &events,
            &RollupOptions {
                by: RollupBy::Day,
                tz: Some(tz.to_owned()),
                block_ms: None,
            },
        )
    }

    #[test]
    fn tokyo_buckets_near_midnight_forward() {
        // 23:30Z on Aug 1 is 08:30 Aug 2 in Tokyo (UTC+9).
        let out = day_rollup("Asia/Tokyo", vec![event("m", "2026-08-01T23:30:00.000Z")]);
        assert_eq!(out[0].key, "2026-08-02");
    }

    #[test]
    fn los_angeles_buckets_near_midnight_backward() {
        // 05:00Z on Aug 2 is 22:00 Aug 1 in Los Angeles (UTC-7, DST).
        let out = day_rollup(
            "America/Los_Angeles",
            vec![event("m", "2026-08-02T05:00:00.000Z")],
        );
        assert_eq!(out[0].key, "2026-08-01");
    }

    #[test]
    fn kolkata_half_hour_offset() {
        // 18:45Z on Jan 1 is 00:15 Jan 2 in Kolkata (UTC+5:30).
        let out = day_rollup("Asia/Kolkata", vec![event("m", "2026-01-01T18:45:00.000Z")]);
        assert_eq!(out[0].key, "2026-01-02");
    }

    #[test]
    fn new_york_spring_forward_transition() {
        // 2026-03-08 spring-forward in New York: clocks jump 02:00 -> 03:00 EST.
        // 06:30Z is 01:30 EST (still Mar 8), 07:30Z is 03:30 EDT (still Mar 8).
        let out = day_rollup(
            "America/New_York",
            vec![
                event("m", "2026-03-08T06:30:00.000Z"),
                event("m", "2026-03-08T07:30:00.000Z"),
            ],
        );
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].key, "2026-03-08");
        assert_eq!(out[0].events, 2);
    }

    #[test]
    fn new_york_fall_back_transition() {
        // 2026-11-01 fall-back in New York: 04:30Z is 00:30 EDT (Nov 1),
        // 05:30Z is 01:30 (the repeated hour, still Nov 1), 08:30Z is 03:30 EST
        // (Nov 1). All three land on Nov 1 local.
        let out = day_rollup(
            "America/New_York",
            vec![
                event("m", "2026-11-01T04:30:00.000Z"),
                event("m", "2026-11-01T05:30:00.000Z"),
                event("m", "2026-11-01T08:30:00.000Z"),
            ],
        );
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].key, "2026-11-01");
        assert_eq!(out[0].events, 3);
    }

    #[test]
    fn omitted_tz_matches_explicit_system_zone() {
        // The tz-omitted default must resolve the SAME zone the system reports,
        // so an omitted-tz day rollup equals one that names the system zone
        // explicitly. Portable: it asserts equality between the two calls rather
        // than pinning any particular host zone.
        let events = vec![
            event("m", "2026-08-01T23:30:00.000Z"),
            event("m", "2026-08-02T00:30:00.000Z"),
            event("m", "2026-08-02T12:00:00.000Z"),
        ];
        let omitted = rollup(
            &events,
            &RollupOptions {
                by: RollupBy::Day,
                tz: None,
                block_ms: None,
            },
        );
        if let Some(name) = jiff::tz::TimeZone::system().iana_name() {
            let explicit = day_rollup(name, events.clone());
            assert_eq!(
                omitted.len(),
                explicit.len(),
                "omitted-tz and explicit-system-zone bucket counts differ"
            );
            for (o, e) in omitted.iter().zip(explicit.iter()) {
                assert_eq!(o.key, e.key, "omitted vs explicit system-zone key");
                assert_eq!(o.events, e.events, "omitted vs explicit system-zone events");
            }
        } else {
            // A system zone with no IANA name reports UTC; assert the fallback.
            let utc = day_rollup("UTC", events.clone());
            for (o, u) in omitted.iter().zip(utc.iter()) {
                assert_eq!(o.key, u.key);
            }
        }
    }

    #[test]
    fn unknown_tz_falls_back_to_utc() {
        // An unknown zone degrades to the UTC calendar date.
        let out = day_rollup("Not/AZone", vec![event("m", "2026-01-01T23:30:00.000Z")]);
        assert_eq!(out[0].key, "2026-01-01");
    }

    #[test]
    fn invalid_timestamp_day_key() {
        let out = rollup(
            &[event("m", "not-a-date")],
            &RollupOptions::new(RollupBy::Day),
        );
        assert_eq!(out[0].key, "invalid-date");
    }

    #[test]
    fn cache_write1h_clamped_per_event() {
        let mut e1 = event("m", "2026-01-01T00:00:00.000Z");
        e1.tokens.cache_write = 5;
        e1.tokens.cache_write1h = Some(100); // over its own cacheWrite -> clamp to 5
        let mut e2 = event("m", "2026-01-01T00:00:00.000Z");
        e2.tokens.cache_write = 8;
        e2.tokens.cache_write1h = Some(3);
        let out = rollup(&[e1, e2], &RollupOptions::new(RollupBy::Model));
        assert_eq!(out[0].tokens.cache_write, 13);
        assert_eq!(out[0].tokens.cache_write1h, Some(8)); // 5 + 3
    }

    #[test]
    fn calls_default_to_one_and_cost_sums_finite() {
        let mut e1 = event("m", "2026-01-01T00:00:00.000Z");
        e1.calls = Some(4);
        e1.cost_usd = Some(0.01);
        let mut e2 = event("m", "2026-01-01T00:00:00.000Z");
        e2.calls = None; // -> 1
        e2.cost_usd = Some(f64::INFINITY); // non-finite -> ignored
        let out = rollup(&[e1, e2], &RollupOptions::new(RollupBy::Model));
        assert_eq!(out[0].calls, 5);
        assert_eq!(out[0].cost_usd, Some(0.01));
    }

    #[test]
    fn no_cost_stays_absent() {
        let out = rollup(
            &[event("m", "2026-01-01T00:00:00.000Z")],
            &RollupOptions::new(RollupBy::Model),
        );
        assert_eq!(out[0].cost_usd, None);
    }

    #[test]
    fn workspace_unknown_fallback() {
        let out = rollup(
            &[event("m", "2026-01-01T00:00:00.000Z")],
            &RollupOptions::new(RollupBy::Workspace),
        );
        assert_eq!(out[0].key, "(unknown)");
    }

    fn block_rollup_utc(events: Vec<UsageEvent>, block_ms: Option<i64>) -> Vec<Rollup> {
        rollup(
            &events,
            &RollupOptions {
                by: RollupBy::Block,
                tz: Some("UTC".to_owned()),
                block_ms,
            },
        )
    }

    #[test]
    fn block_anchors_to_the_hour() {
        // First event at 09:17 opens a block floored to 09:00; a later event
        // within 5h joins it.
        let out = block_rollup_utc(
            vec![
                event("m", "2026-01-01T09:17:00.000Z"),
                event("m", "2026-01-01T11:00:00.000Z"),
            ],
            None,
        );
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].key, "2026-01-01T09:00:00.000Z");
        assert_eq!(out[0].events, 2);
    }

    #[test]
    fn block_splits_after_window() {
        // 09:00 block covers [09:00, 14:00). An event at 14:30 opens a new block
        // floored to 14:00.
        let out = block_rollup_utc(
            vec![
                event("m", "2026-01-01T09:00:00.000Z"),
                event("m", "2026-01-01T14:30:00.000Z"),
            ],
            None,
        );
        assert_eq!(out.len(), 2);
        assert_eq!(out[0].key, "2026-01-01T09:00:00.000Z");
        assert_eq!(out[1].key, "2026-01-01T14:00:00.000Z");
    }

    #[test]
    fn block_boundary_event_opens_next_block() {
        // The event exactly at start+blockMs (14:00) is NOT in [09:00, 14:00).
        let out = block_rollup_utc(
            vec![
                event("m", "2026-01-01T09:00:00.000Z"),
                event("m", "2026-01-01T14:00:00.000Z"),
            ],
            None,
        );
        assert_eq!(out.len(), 2);
        assert_eq!(out[0].key, "2026-01-01T09:00:00.000Z");
        assert_eq!(out[0].events, 1);
        assert_eq!(out[1].key, "2026-01-01T14:00:00.000Z");
        assert_eq!(out[1].events, 1);
    }

    #[test]
    fn block_idle_gap_larger_than_window() {
        // A gap far beyond one block opens a fresh block anchored to the later
        // event's hour.
        let out = block_rollup_utc(
            vec![
                event("m", "2026-01-01T09:00:00.000Z"),
                event("m", "2026-01-02T20:45:00.000Z"),
            ],
            None,
        );
        assert_eq!(out.len(), 2);
        assert_eq!(out[0].key, "2026-01-01T09:00:00.000Z");
        assert_eq!(out[1].key, "2026-01-02T20:00:00.000Z");
    }

    #[test]
    fn block_custom_width() {
        // A 1h block: 09:17 anchors to 09:00, [09:00, 10:00); 10:05 opens a new
        // block at 10:00.
        let out = block_rollup_utc(
            vec![
                event("m", "2026-01-01T09:17:00.000Z"),
                event("m", "2026-01-01T10:05:00.000Z"),
            ],
            Some(3_600_000),
        );
        assert_eq!(out.len(), 2);
        assert_eq!(out[0].key, "2026-01-01T09:00:00.000Z");
        assert_eq!(out[1].key, "2026-01-01T10:00:00.000Z");
    }

    #[test]
    fn block_orders_unsorted_input() {
        // Events arrive out of order; they are sorted before windowing.
        let out = block_rollup_utc(
            vec![
                event("m", "2026-01-01T14:30:00.000Z"),
                event("m", "2026-01-01T09:00:00.000Z"),
            ],
            None,
        );
        assert_eq!(out.len(), 2);
        assert_eq!(out[0].key, "2026-01-01T09:00:00.000Z");
        assert_eq!(out[1].key, "2026-01-01T14:00:00.000Z");
    }

    #[test]
    fn block_omitted_tz_matches_explicit_system_zone() {
        // The tz-omitted block floor must resolve the SAME zone the system
        // reports, so an omitted-tz block rollup equals one that names the
        // system zone explicitly. Portable: it asserts equality between the two
        // calls rather than pinning any particular host zone. Mirrors the
        // TypeScript omitted-vs-resolved-system parity test.
        let events = vec![
            event("m", "2026-01-01T09:17:00.000Z"),
            event("m", "2026-01-01T13:00:00.000Z"),
            event("m", "2026-01-01T20:45:00.000Z"),
        ];
        let omitted = rollup(
            &events,
            &RollupOptions {
                by: RollupBy::Block,
                tz: None,
                block_ms: None,
            },
        );
        if let Some(name) = jiff::tz::TimeZone::system().iana_name() {
            let explicit = rollup(
                &events,
                &RollupOptions {
                    by: RollupBy::Block,
                    tz: Some(name.to_owned()),
                    block_ms: None,
                },
            );
            assert_eq!(
                omitted.len(),
                explicit.len(),
                "omitted-tz and explicit-system-zone block counts differ"
            );
            for (o, e) in omitted.iter().zip(explicit.iter()) {
                assert_eq!(o.key, e.key, "omitted vs explicit system-zone block key");
                assert_eq!(
                    o.events, e.events,
                    "omitted vs explicit system-zone block events"
                );
            }
        } else {
            // A system zone with no IANA name reports UTC; assert the fallback.
            let utc = block_rollup_utc(events.clone(), None);
            for (o, u) in omitted.iter().zip(utc.iter()) {
                assert_eq!(o.key, u.key);
            }
        }
    }

    #[test]
    fn block_half_hour_offset_zone_floors_local_hour() {
        // Kolkata is UTC+5:30. 09:17Z is 14:47 local, floored to 14:00 local =
        // 08:30Z. The key is that UTC instant.
        let out = rollup(
            &[event("m", "2026-01-01T09:17:00.000Z")],
            &RollupOptions {
                by: RollupBy::Block,
                tz: Some("Asia/Kolkata".to_owned()),
                block_ms: None,
            },
        );
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].key, "2026-01-01T08:30:00.000Z");
    }

    #[test]
    fn block_extreme_width_saturates_and_never_splits() {
        // A blockMs near i64::MAX must not panic (debug) or wrap (release); the
        // saturated block end keeps every later event in the first block.
        let out = block_rollup_utc(
            vec![
                event("m", "2026-01-01T09:00:00.000Z"),
                event("m", "2026-01-01T14:00:00.000Z"),
                event("m", "2030-06-01T00:00:00.000Z"),
            ],
            Some(i64::MAX),
        );
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].key, "2026-01-01T09:00:00.000Z");
        assert_eq!(out[0].events, 3);
    }

    #[test]
    fn block_new_york_fall_back_preserves_both_occurrences() {
        // 2026-11-01 fall-back in New York: 05:30Z is the first 01:30 (EDT) and
        // 06:30Z is the second 01:30 (EST). Occurrence-preserving flooring keeps
        // them in separate blocks anchored to 05:00Z and 06:00Z respectively (a
        // one-hour block width isolates each). Matches the TS reference.
        let out = rollup(
            &[
                event("m", "2026-11-01T05:30:00.000Z"),
                event("m", "2026-11-01T06:30:00.000Z"),
            ],
            &RollupOptions {
                by: RollupBy::Block,
                tz: Some("America/New_York".to_owned()),
                block_ms: Some(3_600_000),
            },
        );
        assert_eq!(out.len(), 2);
        assert_eq!(out[0].key, "2026-11-01T05:00:00.000Z");
        assert_eq!(out[1].key, "2026-11-01T06:00:00.000Z");
    }

    #[test]
    fn block_invalid_timestamps_share_a_bucket() {
        let out = block_rollup_utc(
            vec![
                event("m", "2026-01-01T09:00:00.000Z"),
                event("m", "not-a-date"),
                event("m", "also-bad"),
            ],
            None,
        );
        assert_eq!(out.len(), 2);
        assert_eq!(out[0].key, "2026-01-01T09:00:00.000Z");
        assert_eq!(out[1].key, "invalid-date");
        assert_eq!(out[1].events, 2);
    }
}
