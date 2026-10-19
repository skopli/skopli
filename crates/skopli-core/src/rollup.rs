//! Aggregation of [`UsageEvent`]s into keyed [`Rollup`]s; the twin of
//! `src/rollup.ts`.
//!
//! Keyed rollups group events by one dimension and come back sorted by key,
//! bytewise. Day keys are the calendar date in the requested IANA zone via the
//! bundled tzdb, so every platform agrees; an unknown zone falls back to UTC
//! and an unparseable timestamp lands in the `invalid-date` row. Block rollups
//! are session windows: events sorted by instant, a block opening at the first
//! event's instant floored to the width grain in the zone and spanning the
//! width. The grain is the width itself below one hour and the hour otherwise,
//! so a block never starts mid-hour unless the width is shorter than an hour,
//! and two blocks never share a key.

use jiff::tz::TimeZone;
use serde::Serialize;

use crate::time::{date_parse_ms, to_iso_string};
use crate::types::{TokenCounts, UsageEvent};

const INVALID_DATE: &str = "invalid-date";
const HOUR_MS: i64 = 3_600_000;

/// The dimension a [`rollup`] groups by.
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
    /// The dimension named on the wire (`"model"`, `"day"`, ...), or `None`.
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

#[derive(Debug, Clone)]
pub struct RollupOptions {
    pub by: RollupBy,
    /// IANA zone for day keys and block starts. `None` is the system zone,
    /// an unknown name falls back to UTC.
    pub tz: Option<String>,
    /// Billing-block width in milliseconds; `None` or a non-positive value uses
    /// [`DEFAULT_BLOCK_MS`].
    pub block_ms: Option<i64>,
}

impl RollupOptions {
    pub fn new(by: RollupBy) -> Self {
        Self {
            by,
            tz: None,
            block_ms: None,
        }
    }
}

/// One aggregated row. Counters saturate at `u64::MAX` rather than wrapping.
/// `cost_usd` is omitted, never `null`, when no event in the row carried a
/// finite cost.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Rollup {
    pub key: String,
    pub tokens: TokenCounts,
    pub events: u64,
    pub turns: u64,
    /// Underlying model calls; may exceed `events` for session-aggregate
    /// sources. An event without a reported count contributes one.
    pub calls: u64,
    #[serde(rename = "costUsd", skip_serializing_if = "Option::is_none")]
    pub cost_usd: Option<f64>,
}

impl Rollup {
    fn empty(key: String) -> Self {
        Self {
            key,
            tokens: TokenCounts::default(),
            events: 0,
            turns: 0,
            calls: 0,
            cost_usd: None,
        }
    }

    fn add(&mut self, event: &UsageEvent) {
        add_tokens(&mut self.tokens, &event.tokens);
        self.events = self.events.saturating_add(1);
        self.turns = self.turns.saturating_add(u64::from(event.turn));
        self.calls = self.calls.saturating_add(event.calls.unwrap_or(1));
        if let Some(cost) = event.cost_usd.filter(|c| c.is_finite()) {
            self.cost_usd = Some(self.cost_usd.unwrap_or(0.0) + cost);
        }
    }
}

/// Add one event's counters to a running sum. Every counter saturates at
/// `u64::MAX` instead of wrapping, so a corrupt source can pin a total at the
/// ceiling but never shrink it. Each event's `cache_write1h` is clamped to its
/// own `cache_write` first, so one malformed event cannot rebill other events'
/// writes at the 1h rate.
pub fn add_tokens(sum: &mut TokenCounts, event: &TokenCounts) {
    sum.input = sum.input.saturating_add(event.input);
    sum.output = sum.output.saturating_add(event.output);
    sum.cache_read = sum.cache_read.saturating_add(event.cache_read);
    sum.cache_write = sum.cache_write.saturating_add(event.cache_write);
    sum.reasoning = sum.reasoning.saturating_add(event.reasoning);
    if let Some(write1h) = event.cache_write1h {
        let clamped = write1h.min(event.cache_write);
        sum.cache_write1h = Some(sum.cache_write1h.unwrap_or(0).saturating_add(clamped));
    }
}

/// The zone to do civil-time arithmetic in: `None` for UTC, which the integer
/// paths handle directly, and for an unknown zone name, which falls back to UTC.
fn zone(tz: Option<&str>) -> Option<TimeZone> {
    match tz {
        Some("UTC") => None,
        None => Some(TimeZone::system()),
        Some(name) => TimeZone::get(name).ok(),
    }
}

/// The `YYYY-MM-DD` calendar date of `timestamp` in `tz`, or `invalid-date`.
fn day_key(timestamp: &str, tz: Option<&str>) -> String {
    let Some(ms) = date_parse_ms(timestamp) else {
        return INVALID_DATE.to_owned();
    };
    let local = zone(tz).and_then(|zone| {
        let date = zone
            .to_datetime(jiff::Timestamp::from_millisecond(ms).ok()?)
            .date();
        Some(format!(
            "{:04}-{:02}-{:02}",
            date.year(),
            date.month(),
            date.day()
        ))
    });
    local.unwrap_or_else(|| utc_day_key(ms))
}

fn utc_day_key(ms: i64) -> String {
    to_iso_string(ms).map_or_else(
        || INVALID_DATE.to_owned(),
        |iso| iso[..iso.find('T').unwrap_or(iso.len())].to_owned(),
    )
}

/// The rollup key for `event` under `options`. Blocks are keyed by window, not
/// per event, so [`RollupBy::Block`] gives the day key here.
pub fn rollup_key(event: &UsageEvent, options: &RollupOptions) -> String {
    match options.by {
        RollupBy::Model => event.model.clone(),
        RollupBy::Session => event.session_id.clone(),
        RollupBy::Harness => event.harness.clone(),
        RollupBy::Workspace => event
            .workspace
            .clone()
            .unwrap_or_else(|| "(unknown)".to_owned()),
        RollupBy::Day | RollupBy::Block => day_key(&event.timestamp, options.tz.as_deref()),
    }
}

/// Milliseconds from the start of the wall-clock hour in `tz` to `ms`.
/// Subtracting it from the instant keeps the occurrence of a repeated
/// (fall-back) hour; re-resolving the floored civil time would collapse both
/// occurrences onto the earlier one. Out-of-range instants use the UTC hour.
fn within_hour(ms: i64, tz: Option<&str>) -> i64 {
    let local = zone(tz).and_then(|zone| {
        let time = zone
            .to_datetime(jiff::Timestamp::from_millisecond(ms).ok()?)
            .time();
        Some(
            i64::from(time.minute()) * 60_000
                + i64::from(time.second()) * 1000
                + ms.rem_euclid(1000),
        )
    });
    local.unwrap_or_else(|| ms.rem_euclid(HOUR_MS))
}

/// The start of the block an event at `ms` opens: the instant floored to the
/// width grain, measured from the start of its wall-clock hour (`within_hour`
/// milliseconds earlier). The grain is `width` below one hour and the hour
/// otherwise, so consecutive blocks always have distinct, ascending starts.
pub fn block_start(ms: i64, within_hour: i64, width: i64) -> i64 {
    ms - within_hour % width.min(HOUR_MS)
}

/// The first instant past a block: `start + width`, saturating so an extreme
/// width never wraps and simply never splits.
pub fn block_end(start: i64, width: i64) -> i64 {
    start.saturating_add(width)
}

fn block_rollup(events: &[UsageEvent], options: &RollupOptions) -> Vec<Rollup> {
    let width = options
        .block_ms
        .filter(|w| *w > 0)
        .unwrap_or(DEFAULT_BLOCK_MS);
    let tz = options.tz.as_deref();

    let mut ordered: Vec<(&UsageEvent, Option<i64>)> = events
        .iter()
        .map(|e| (e, date_parse_ms(&e.timestamp)))
        .collect();
    ordered.sort_by_key(|(_, ms)| (ms.is_none(), ms.unwrap_or(0)));

    let mut out: Vec<Rollup> = Vec::new();
    let mut end: Option<i64> = None;
    for (event, ms) in ordered {
        let key = match ms {
            None => {
                (out.last().is_none_or(|r| r.key != INVALID_DATE)).then(|| INVALID_DATE.to_owned())
            }
            Some(ms) if end.is_none_or(|end| ms >= end) => {
                let start = block_start(ms, within_hour(ms, tz), width);
                end = Some(block_end(start, width));
                Some(to_iso_string(start).unwrap_or_else(|| INVALID_DATE.to_owned()))
            }
            Some(_) => None,
        };
        if let Some(key) = key {
            out.push(Rollup::empty(key));
        }
        out.last_mut().expect("a block is open").add(event);
    }
    out
}

/// Aggregate `events` into rows keyed by `options.by`, sorted by key.
pub fn rollup(events: &[UsageEvent], options: &RollupOptions) -> Vec<Rollup> {
    if options.by == RollupBy::Block {
        return block_rollup(events, options);
    }
    let mut rows: std::collections::BTreeMap<String, Rollup> = std::collections::BTreeMap::new();
    for event in events {
        let key = rollup_key(event, options);
        rows.entry(key.clone())
            .or_insert_with(|| Rollup::empty(key))
            .add(event);
    }
    rows.into_values().collect()
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

    fn opts(by: RollupBy, tz: Option<&str>, block_ms: Option<i64>) -> RollupOptions {
        RollupOptions {
            by,
            tz: tz.map(str::to_owned),
            block_ms,
        }
    }

    fn day_rollup(tz: &str, events: &[UsageEvent]) -> Vec<Rollup> {
        rollup(events, &opts(RollupBy::Day, Some(tz), None))
    }

    fn block_rollup_utc(events: &[UsageEvent], block_ms: Option<i64>) -> Vec<Rollup> {
        rollup(events, &opts(RollupBy::Block, Some("UTC"), block_ms))
    }

    fn keys(out: &[Rollup]) -> Vec<&str> {
        out.iter().map(|r| r.key.as_str()).collect()
    }

    #[test]
    fn groups_by_model_sorted() {
        let events = [
            event("zeta", "2026-01-01T00:00:00.000Z"),
            event("alpha", "2026-01-01T00:00:00.000Z"),
            event("alpha", "2026-01-01T00:00:00.000Z"),
        ];
        let out = rollup(&events, &RollupOptions::new(RollupBy::Model));
        assert_eq!(keys(&out), ["alpha", "zeta"]);
        assert_eq!(out[0].events, 2);
        assert_eq!(out[0].tokens.input, 2);
        assert_eq!(out[1].events, 1);
    }

    #[test]
    fn day_bucketing_is_utc() {
        let events = [
            event("m", "2026-01-01T23:30:00.000Z"),
            event("m", "2026-01-02T00:30:00.000Z"),
        ];
        assert_eq!(
            keys(&day_rollup("UTC", &events)),
            ["2026-01-01", "2026-01-02"]
        );
    }

    #[test]
    fn tokyo_buckets_near_midnight_forward() {
        let out = day_rollup("Asia/Tokyo", &[event("m", "2026-08-01T23:30:00.000Z")]);
        assert_eq!(out[0].key, "2026-08-02");
    }

    #[test]
    fn los_angeles_buckets_near_midnight_backward() {
        let out = day_rollup(
            "America/Los_Angeles",
            &[event("m", "2026-08-02T05:00:00.000Z")],
        );
        assert_eq!(out[0].key, "2026-08-01");
    }

    #[test]
    fn kolkata_half_hour_offset() {
        let out = day_rollup("Asia/Kolkata", &[event("m", "2026-01-01T18:45:00.000Z")]);
        assert_eq!(out[0].key, "2026-01-02");
    }

    #[test]
    fn new_york_spring_forward_transition() {
        let out = day_rollup(
            "America/New_York",
            &[
                event("m", "2026-03-08T06:30:00.000Z"),
                event("m", "2026-03-08T07:30:00.000Z"),
            ],
        );
        assert_eq!(keys(&out), ["2026-03-08"]);
        assert_eq!(out[0].events, 2);
    }

    #[test]
    fn new_york_fall_back_transition() {
        let out = day_rollup(
            "America/New_York",
            &[
                event("m", "2026-11-01T04:30:00.000Z"),
                event("m", "2026-11-01T05:30:00.000Z"),
                event("m", "2026-11-01T08:30:00.000Z"),
            ],
        );
        assert_eq!(keys(&out), ["2026-11-01"]);
        assert_eq!(out[0].events, 3);
    }

    fn assert_omitted_tz_matches_system_zone(by: RollupBy, events: &[UsageEvent]) {
        let omitted = rollup(events, &opts(by, None, None));
        let system = TimeZone::system();
        let explicit = rollup(
            events,
            &opts(by, Some(system.iana_name().unwrap_or("UTC")), None),
        );
        assert_eq!(keys(&omitted), keys(&explicit));
        for (o, e) in omitted.iter().zip(&explicit) {
            assert_eq!(o.events, e.events, "key {}", o.key);
        }
    }

    #[test]
    fn omitted_tz_matches_explicit_system_zone() {
        assert_omitted_tz_matches_system_zone(
            RollupBy::Day,
            &[
                event("m", "2026-08-01T23:30:00.000Z"),
                event("m", "2026-08-02T00:30:00.000Z"),
                event("m", "2026-08-02T12:00:00.000Z"),
            ],
        );
    }

    #[test]
    fn unknown_tz_falls_back_to_utc() {
        let out = day_rollup("Not/AZone", &[event("m", "2026-01-01T23:30:00.000Z")]);
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
        e1.tokens.cache_write1h = Some(100);
        let mut e2 = event("m", "2026-01-01T00:00:00.000Z");
        e2.tokens.cache_write = 8;
        e2.tokens.cache_write1h = Some(3);
        let out = rollup(&[e1, e2], &RollupOptions::new(RollupBy::Model));
        assert_eq!(out[0].tokens.cache_write, 13);
        assert_eq!(out[0].tokens.cache_write1h, Some(8));
    }

    #[test]
    fn counters_saturate_instead_of_wrapping() {
        let mut e1 = event("m", "2026-01-01T00:00:00.000Z");
        e1.tokens.input = u64::MAX;
        e1.tokens.cache_write = u64::MAX;
        e1.tokens.cache_write1h = Some(u64::MAX);
        e1.calls = Some(u64::MAX);
        let mut e2 = event("m", "2026-01-01T00:00:00.000Z");
        e2.tokens.cache_write1h = Some(10);
        e2.calls = Some(1);
        let out = rollup(&[e1, e2], &RollupOptions::new(RollupBy::Model));
        assert_eq!(out[0].tokens.input, u64::MAX);
        assert_eq!(out[0].tokens.cache_write, u64::MAX);
        assert_eq!(out[0].tokens.cache_write1h, Some(u64::MAX));
        assert_eq!(out[0].calls, u64::MAX);
        assert_eq!(out[0].tokens.output, 4);
    }

    #[test]
    fn calls_default_to_one_and_cost_sums_finite() {
        let mut e1 = event("m", "2026-01-01T00:00:00.000Z");
        e1.calls = Some(4);
        e1.cost_usd = Some(0.01);
        let mut e2 = event("m", "2026-01-01T00:00:00.000Z");
        e2.cost_usd = Some(f64::INFINITY);
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

    #[test]
    fn block_anchors_to_the_hour() {
        let out = block_rollup_utc(
            &[
                event("m", "2026-01-01T09:17:00.000Z"),
                event("m", "2026-01-01T11:00:00.000Z"),
            ],
            None,
        );
        assert_eq!(keys(&out), ["2026-01-01T09:00:00.000Z"]);
        assert_eq!(out[0].events, 2);
    }

    #[test]
    fn block_splits_after_window() {
        let out = block_rollup_utc(
            &[
                event("m", "2026-01-01T09:00:00.000Z"),
                event("m", "2026-01-01T14:30:00.000Z"),
            ],
            None,
        );
        assert_eq!(
            keys(&out),
            ["2026-01-01T09:00:00.000Z", "2026-01-01T14:00:00.000Z"]
        );
    }

    #[test]
    fn block_boundary_event_opens_next_block() {
        let out = block_rollup_utc(
            &[
                event("m", "2026-01-01T09:00:00.000Z"),
                event("m", "2026-01-01T14:00:00.000Z"),
            ],
            None,
        );
        assert_eq!(
            keys(&out),
            ["2026-01-01T09:00:00.000Z", "2026-01-01T14:00:00.000Z"]
        );
        assert_eq!((out[0].events, out[1].events), (1, 1));
    }

    #[test]
    fn block_idle_gap_larger_than_window() {
        let out = block_rollup_utc(
            &[
                event("m", "2026-01-01T09:00:00.000Z"),
                event("m", "2026-01-02T20:45:00.000Z"),
            ],
            None,
        );
        assert_eq!(
            keys(&out),
            ["2026-01-01T09:00:00.000Z", "2026-01-02T20:00:00.000Z"]
        );
    }

    #[test]
    fn block_custom_width() {
        let out = block_rollup_utc(
            &[
                event("m", "2026-01-01T09:17:00.000Z"),
                event("m", "2026-01-01T10:05:00.000Z"),
            ],
            Some(HOUR_MS),
        );
        assert_eq!(
            keys(&out),
            ["2026-01-01T09:00:00.000Z", "2026-01-01T10:00:00.000Z"]
        );
    }

    #[test]
    fn block_sub_hour_width_gives_each_block_its_own_key() {
        let events = [
            event("m", "2026-03-01T22:10:00.000Z"),
            event("m", "2026-03-01T22:40:00.000Z"),
        ];
        let half_hour = block_rollup_utc(&events, Some(30 * 60_000));
        assert_eq!(
            keys(&half_hour),
            ["2026-03-01T22:00:00.000Z", "2026-03-01T22:30:00.000Z"]
        );
        let forty_minutes = block_rollup_utc(&events, Some(40 * 60_000));
        assert_eq!(
            keys(&forty_minutes),
            ["2026-03-01T22:00:00.000Z", "2026-03-01T22:40:00.000Z"]
        );
    }

    #[test]
    fn block_orders_unsorted_input() {
        let out = block_rollup_utc(
            &[
                event("m", "2026-01-01T14:30:00.000Z"),
                event("m", "2026-01-01T09:00:00.000Z"),
            ],
            None,
        );
        assert_eq!(
            keys(&out),
            ["2026-01-01T09:00:00.000Z", "2026-01-01T14:00:00.000Z"]
        );
    }

    #[test]
    fn block_omitted_tz_matches_explicit_system_zone() {
        assert_omitted_tz_matches_system_zone(
            RollupBy::Block,
            &[
                event("m", "2026-01-01T09:17:00.000Z"),
                event("m", "2026-01-01T13:00:00.000Z"),
                event("m", "2026-01-01T20:45:00.000Z"),
            ],
        );
    }

    #[test]
    fn block_half_hour_offset_zone_floors_local_hour() {
        // 09:17Z is 14:47 in Kolkata (UTC+5:30); 14:00 local is 08:30Z.
        let out = rollup(
            &[event("m", "2026-01-01T09:17:00.000Z")],
            &opts(RollupBy::Block, Some("Asia/Kolkata"), None),
        );
        assert_eq!(keys(&out), ["2026-01-01T08:30:00.000Z"]);
    }

    #[test]
    fn block_extreme_width_saturates_and_never_splits() {
        let out = block_rollup_utc(
            &[
                event("m", "2026-01-01T09:00:00.000Z"),
                event("m", "2026-01-01T14:00:00.000Z"),
                event("m", "2030-06-01T00:00:00.000Z"),
            ],
            Some(i64::MAX),
        );
        assert_eq!(keys(&out), ["2026-01-01T09:00:00.000Z"]);
        assert_eq!(out[0].events, 3);
    }

    #[test]
    fn block_new_york_fall_back_preserves_both_occurrences() {
        // 05:30Z is the first 01:30 (EDT) and 06:30Z the second (EST).
        let out = rollup(
            &[
                event("m", "2026-11-01T05:30:00.000Z"),
                event("m", "2026-11-01T06:30:00.000Z"),
            ],
            &opts(RollupBy::Block, Some("America/New_York"), Some(HOUR_MS)),
        );
        assert_eq!(
            keys(&out),
            ["2026-11-01T05:00:00.000Z", "2026-11-01T06:00:00.000Z"]
        );
    }

    #[test]
    fn block_invalid_timestamps_share_a_bucket() {
        let out = block_rollup_utc(
            &[
                event("m", "2026-01-01T09:00:00.000Z"),
                event("m", "not-a-date"),
                event("m", "also-bad"),
            ],
            None,
        );
        assert_eq!(keys(&out), ["2026-01-01T09:00:00.000Z", "invalid-date"]);
        assert_eq!(out[1].events, 2);
    }
}
