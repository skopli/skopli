//! Bounded exhaustive check of `skopli_core::rollup`: every sequence of up to
//! `MAX_EVENTS` events drawn from the finite domain below is rolled up, and the
//! result is checked against the promises in `.agents/skills/verify-skopli-core/SKILL.md`.
//! Expected keys and instants are literals worked out from the calendar, never
//! from the crate under test.

use skopli_core::rollup::{Rollup, RollupBy, RollupOptions};
use skopli_core::types::{TokenCounts, UsageEvent};

#[cfg(kani)]
mod kani_proofs;

pub const MAX_EVENTS: usize = 3;
pub const KEYS: [&str; 3] = ["b", "a", "c"];
pub const HOUR_MS: i64 = 3_600_000;
/// The five-hour default from `golden/rollup-block/README.md`.
pub const DEFAULT_BLOCK_MS: i64 = 18_000_000;

/// One timestamp with its epoch milliseconds, UTC hour floor and calendar day
/// in each zone, all written by hand. New York is on EST (UTC-5) on these dates.
#[derive(Clone, Copy)]
pub struct Instant {
    pub iso: &'static str,
    pub ms: Option<i64>,
    pub hour: &'static str,
    pub day_utc: &'static str,
    pub day_ny: &'static str,
}

const INVALID: &str = "invalid-date";

/// Instants around the UTC hour, day and five-hour block edges, the New York
/// midnight, and one that does not parse.
pub const INSTANTS: [Instant; 9] = [
    Instant {
        iso: "2026-03-01T22:30:00.000Z",
        ms: Some(1_772_404_200_000),
        hour: "2026-03-01T22:00:00.000Z",
        day_utc: "2026-03-01",
        day_ny: "2026-03-01",
    },
    Instant {
        iso: "2026-03-01T22:59:59.999Z",
        ms: Some(1_772_405_999_999),
        hour: "2026-03-01T22:00:00.000Z",
        day_utc: "2026-03-01",
        day_ny: "2026-03-01",
    },
    Instant {
        iso: "2026-03-01T23:00:00.000Z",
        ms: Some(1_772_406_000_000),
        hour: "2026-03-01T23:00:00.000Z",
        day_utc: "2026-03-01",
        day_ny: "2026-03-01",
    },
    Instant {
        iso: "2026-03-02T00:00:00.000Z",
        ms: Some(1_772_409_600_000),
        hour: "2026-03-02T00:00:00.000Z",
        day_utc: "2026-03-02",
        day_ny: "2026-03-01",
    },
    Instant {
        iso: "2026-03-02T03:59:59.999Z",
        ms: Some(1_772_423_999_999),
        hour: "2026-03-02T03:00:00.000Z",
        day_utc: "2026-03-02",
        day_ny: "2026-03-01",
    },
    Instant {
        iso: "2026-03-02T04:00:00.000Z",
        ms: Some(1_772_424_000_000),
        hour: "2026-03-02T04:00:00.000Z",
        day_utc: "2026-03-02",
        day_ny: "2026-03-01",
    },
    Instant {
        iso: "2026-03-02T04:59:59.999Z",
        ms: Some(1_772_427_599_999),
        hour: "2026-03-02T04:00:00.000Z",
        day_utc: "2026-03-02",
        day_ny: "2026-03-01",
    },
    Instant {
        iso: "2026-03-02T05:00:00.000Z",
        ms: Some(1_772_427_600_000),
        hour: "2026-03-02T05:00:00.000Z",
        day_utc: "2026-03-02",
        day_ny: "2026-03-02",
    },
    Instant {
        iso: "not-a-date",
        ms: None,
        hour: INVALID,
        day_utc: INVALID,
        day_ny: INVALID,
    },
];
pub const BLOCK_WIDTHS: [Option<i64>; 5] = [
    None,
    Some(0),
    Some(HOUR_MS),
    Some(5_400_000),
    Some(1_800_000),
];
pub const BIG: u64 = 1 << 40;

/// One token/calls/cost/turn shape. Costs are dyadic fractions so float sums are exact.
#[derive(Clone, Copy)]
pub struct Pattern {
    pub tokens: [u64; 5],
    pub cache_write1h: Option<u64>,
    pub calls: Option<u64>,
    pub cost_usd: Option<f64>,
    pub turn: bool,
}

pub const PATTERNS: [Pattern; 8] = [
    Pattern {
        tokens: [0; 5],
        cache_write1h: None,
        calls: None,
        cost_usd: None,
        turn: false,
    },
    Pattern {
        tokens: [1, 2, 3, 4, 5],
        cache_write1h: None,
        calls: Some(3),
        cost_usd: Some(0.25),
        turn: true,
    },
    Pattern {
        tokens: [0, 0, 0, 4, 0],
        cache_write1h: Some(2),
        calls: Some(0),
        cost_usd: Some(0.5),
        turn: false,
    },
    Pattern {
        tokens: [0, 0, 0, 4, 0],
        cache_write1h: Some(9),
        calls: None,
        cost_usd: None,
        turn: true,
    },
    Pattern {
        tokens: [0; 5],
        cache_write1h: Some(0),
        calls: Some(1),
        cost_usd: Some(f64::INFINITY),
        turn: false,
    },
    Pattern {
        tokens: [BIG; 5],
        cache_write1h: Some(BIG),
        calls: Some(1 << 20),
        cost_usd: Some(0.125),
        turn: true,
    },
    Pattern {
        tokens: [7, 0, 0, 0, 0],
        cache_write1h: None,
        calls: Some(1),
        cost_usd: Some(f64::NAN),
        turn: false,
    },
    Pattern {
        tokens: [0, 1, 0, 1, 1],
        cache_write1h: Some(1),
        calls: None,
        cost_usd: Some(0.75),
        turn: true,
    },
];
/// The two shapes the time dimensions vary over: a full row and a clamped 1h split.
pub const TIME_PATTERNS: [usize; 2] = [1, 3];

/// Each keyed field gets its own prefix so a rollup that keys on the wrong
/// field is told apart from one that keys on the right one.
pub fn event(key: &str, instant: &str, p: &Pattern) -> UsageEvent {
    let [input, output, cache_read, cache_write, reasoning] = p.tokens;
    UsageEvent {
        harness: format!("h{key}"),
        timestamp: instant.to_owned(),
        session_id: format!("s{key}"),
        message_id: "m".to_owned(),
        turn: p.turn,
        subagent: false,
        model: format!("m{key}"),
        tokens: TokenCounts {
            input,
            output,
            cache_read,
            cache_write,
            cache_write1h: p.cache_write1h,
            reasoning,
        },
        calls: p.calls,
        cost_usd: p.cost_usd,
        workspace: Some(format!("w{key}")),
        title: None,
    }
}

/// Every sequence of `0..=MAX_EVENTS` picks from `variants`.
pub fn sequences<T: Clone>(variants: &[T]) -> Vec<Vec<T>> {
    let mut out = vec![Vec::new()];
    let mut layer = vec![Vec::new()];
    for _ in 0..MAX_EVENTS {
        layer = layer
            .iter()
            .flat_map(|seq: &Vec<T>| {
                variants.iter().map(move |v| {
                    let mut next = seq.clone();
                    next.push(v.clone());
                    next
                })
            })
            .collect();
        out.extend(layer.iter().cloned());
    }
    out
}

pub fn opts(by: RollupBy, tz: &str, block_ms: Option<i64>) -> RollupOptions {
    RollupOptions {
        by,
        tz: Some(tz.to_owned()),
        block_ms,
    }
}

/// The whole domain: keyed dimensions vary key and shape at one instant; day
/// and block vary instant and shape under one key. `Not/A_Zone` is the
/// documented unknown-zone fallback to the UTC calendar date.
pub fn cases() -> Vec<(Vec<UsageEvent>, RollupOptions)> {
    let keyed: Vec<UsageEvent> = KEYS
        .iter()
        .flat_map(|k| PATTERNS.iter().map(|p| event(k, INSTANTS[0].iso, p)))
        .collect();
    let timed: Vec<UsageEvent> = INSTANTS
        .iter()
        .flat_map(|t| {
            TIME_PATTERNS
                .iter()
                .map(|&i| event("k", t.iso, &PATTERNS[i]))
        })
        .collect();
    let mut out = Vec::new();
    for events in sequences(&keyed) {
        for by in [
            RollupBy::Model,
            RollupBy::Session,
            RollupBy::Harness,
            RollupBy::Workspace,
        ] {
            out.push((events.clone(), opts(by, "UTC", None)));
        }
    }
    for events in sequences(&timed) {
        for tz in ["UTC", "America/New_York", "Not/A_Zone"] {
            out.push((events.clone(), opts(RollupBy::Day, tz, None)));
        }
        for width in BLOCK_WIDTHS {
            out.push((events.clone(), opts(RollupBy::Block, "UTC", width)));
        }
    }
    out
}

fn instant(e: &UsageEvent) -> &'static Instant {
    INSTANTS
        .iter()
        .find(|i| i.iso == e.timestamp)
        .expect("every event timestamp is in INSTANTS")
}

/// The key the documentation promises for `e`, from the event's own fields and
/// the literal day table.
pub fn expected_key(e: &UsageEvent, options: &RollupOptions) -> String {
    match options.by {
        RollupBy::Model => e.model.clone(),
        RollupBy::Session => e.session_id.clone(),
        RollupBy::Harness => e.harness.clone(),
        RollupBy::Workspace => e.workspace.clone().expect("domain always sets workspace"),
        RollupBy::Day => match options.tz.as_deref() {
            Some("UTC" | "Not/A_Zone") => instant(e).day_utc,
            Some("America/New_York") => instant(e).day_ny,
            other => panic!("no day table for zone {other:?}"),
        }
        .to_owned(),
        RollupBy::Block => panic!("block keys come from block_spec"),
    }
}

fn sum<T: Into<u128>>(items: impl Iterator<Item = T>) -> u128 {
    items.map(Into::into).sum()
}

/// The saturating-sum promise: rows total exactly the events below the `u64`
/// ceiling; past it every saturated row sits at the ceiling, so the rows total
/// at least the ceiling and never more than the true sum.
fn conserved(want: u128, got: u128) -> bool {
    let ceiling = u128::from(u64::MAX);
    if want <= ceiling {
        want == got
    } else {
        (ceiling..=want).contains(&got)
    }
}

fn clamped1h(e: &UsageEvent) -> u128 {
    e.tokens
        .cache_write1h
        .map_or(0, |v| v.min(e.tokens.cache_write).into())
}

fn finite_cost(e: &UsageEvent) -> Option<f64> {
    e.cost_usd.filter(|c| c.is_finite())
}

/// Promise 1: the rows hold every counter of `events` exactly once and invent
/// none, saturating at `u64::MAX` rather than wrapping. Applied to the whole
/// run and, through `check_row`, to each row with its own events.
pub fn check_conservation<'a>(
    events: impl Iterator<Item = &'a UsageEvent> + Clone,
    out: &[Rollup],
) -> Result<(), String> {
    type Counter = fn(&TokenCounts) -> u64;
    let pairs: [(&str, Counter); 5] = [
        ("input", |t| t.input),
        ("output", |t| t.output),
        ("cacheRead", |t| t.cache_read),
        ("cacheWrite", |t| t.cache_write),
        ("reasoning", |t| t.reasoning),
    ];
    for (name, get) in pairs {
        let want = sum(events.clone().map(|e| get(&e.tokens)));
        let got = sum(out.iter().map(|r| get(&r.tokens)));
        if !conserved(want, got) {
            return Err(format!("{name}: events sum to {want}, rows sum to {got}"));
        }
    }
    let want1h = sum(events.clone().map(clamped1h));
    let got1h = sum(out.iter().map(|r| r.tokens.cache_write1h.unwrap_or(0)));
    if !conserved(want1h, got1h) {
        return Err(format!(
            "cacheWrite1h: clamped events sum to {want1h}, rows sum to {got1h}"
        ));
    }
    if let Some(r) = out.iter().find(|r| {
        r.tokens
            .cache_write1h
            .is_some_and(|v| v > r.tokens.cache_write)
    }) {
        return Err(format!(
            "cacheWrite1h clamp: row {} has cacheWrite1h above cacheWrite",
            r.key
        ));
    }
    if sum(out.iter().map(|r| r.events)) != events.clone().count() as u128 {
        return Err("events: row count differs from input count".to_owned());
    }
    if sum(out.iter().map(|r| r.turns)) != events.clone().filter(|e| e.turn).count() as u128 {
        return Err("turns: rows do not count the turn flags".to_owned());
    }
    let want_calls = sum(events.clone().map(|e| e.calls.unwrap_or(1)));
    let got_calls = sum(out.iter().map(|r| r.calls));
    if !conserved(want_calls, got_calls) {
        return Err(format!(
            "calls: events sum to {want_calls}, rows sum to {got_calls}"
        ));
    }
    let want_cost: f64 = events.filter_map(finite_cost).sum();
    let got_cost: f64 = out.iter().filter_map(|r| r.cost_usd).sum();
    if want_cost != got_cost {
        return Err(format!(
            "costUsd: finite events sum to {want_cost}, rows sum to {got_cost}"
        ));
    }
    Ok(())
}

/// One row holds exactly the counters of the events that belong to it, and
/// carries an optional field exactly when one of those events does.
fn check_row(r: &Rollup, mine: &[&UsageEvent]) -> Result<(), String> {
    check_conservation(mine.iter().copied(), std::slice::from_ref(r))
        .map_err(|m| format!("row {m}; key {}", r.key))?;
    if r.tokens.cache_write1h.is_some() != mine.iter().any(|e| e.tokens.cache_write1h.is_some()) {
        return Err(format!(
            "cacheWrite1h presence: row {} differs from its events",
            r.key
        ));
    }
    if r.cost_usd.is_some() != mine.iter().any(|e| finite_cost(e).is_some()) {
        return Err(format!(
            "costUsd presence: row {} differs from its events",
            r.key
        ));
    }
    Ok(())
}

/// Promise 2: keyed rows are the distinct expected keys in ascending byte
/// order, and each row holds exactly the events with its key.
pub fn check_keyed(
    events: &[UsageEvent],
    options: &RollupOptions,
    out: &[Rollup],
) -> Result<(), String> {
    let mut want: Vec<String> = events.iter().map(|e| expected_key(e, options)).collect();
    want.sort();
    want.dedup();
    let got: Vec<&str> = out.iter().map(|r| r.key.as_str()).collect();
    if want != got {
        return Err(format!("keys: want {want:?}, got {got:?}"));
    }
    for r in out {
        let mine: Vec<&UsageEvent> = events
            .iter()
            .filter(|e| expected_key(e, options) == r.key)
            .collect();
        check_row(r, &mine)?;
    }
    Ok(())
}

/// The documented block rule, written from `golden/rollup-block/README.md`: sort
/// by instant, open a block at the first event floored to the block grain (the
/// width below one hour, the UTC hour otherwise), keep events while
/// `ms < start + width`, and send unparseable instants to one trailing
/// `invalid-date` row. A start on the hour takes its key from the literal table;
/// a sub-hour start is the literal hour plus whole grains, formatted by the
/// core's V8-pinned formatter. Returns each row's key with its member events.
pub fn block_spec(events: &[UsageEvent], width: Option<i64>) -> Vec<(String, Vec<&UsageEvent>)> {
    let width = width.filter(|w| *w > 0).unwrap_or(DEFAULT_BLOCK_MS);
    let mut parsed: Vec<(i64, &Instant, &UsageEvent)> = events
        .iter()
        .filter_map(|e| {
            let i = instant(e);
            i.ms.map(|ms| (ms, i, e))
        })
        .collect();
    parsed.sort_by_key(|(ms, ..)| *ms);
    let mut rows: Vec<(String, Vec<&UsageEvent>)> = Vec::new();
    let mut end: Option<i64> = None;
    for (ms, i, e) in parsed {
        if end.is_none_or(|end| ms >= end) {
            let grains_past_hour = ms.rem_euclid(HOUR_MS) / width.min(HOUR_MS);
            let start = ms - ms.rem_euclid(HOUR_MS) + grains_past_hour * width.min(HOUR_MS);
            end = Some(start.saturating_add(width));
            let key = if grains_past_hour == 0 {
                i.hour.to_owned()
            } else {
                skopli_core::time::to_iso_string(start).expect("in range")
            };
            rows.push((key, Vec::new()));
        }
        rows.last_mut().expect("open block").1.push(e);
    }
    let invalid: Vec<&UsageEvent> = events.iter().filter(|e| instant(e).ms.is_none()).collect();
    if !invalid.is_empty() {
        rows.push((INVALID.to_owned(), invalid));
    }
    rows
}

/// Promise 3: block rows match the documented rule, each holds exactly its
/// member events, and the keys are distinct and ascending. Matching the spec
/// already pins every key to an ISO instant of fixed width, so the order check
/// compares the strings bytewise.
pub fn check_block(
    events: &[UsageEvent],
    options: &RollupOptions,
    out: &[Rollup],
) -> Result<(), String> {
    let want = block_spec(events, options.block_ms);
    let want_keys: Vec<&str> = want.iter().map(|(k, _)| k.as_str()).collect();
    let got_keys: Vec<&str> = out.iter().map(|r| r.key.as_str()).collect();
    if want_keys != got_keys {
        return Err(format!(
            "blocks: spec gives {want_keys:?}, rollup gives {got_keys:?}"
        ));
    }
    for (r, (_, mine)) in out.iter().zip(&want) {
        check_row(r, mine)?;
    }
    let starts: Vec<&str> = out
        .iter()
        .map(|r| r.key.as_str())
        .filter(|k| *k != INVALID)
        .collect();
    if starts.windows(2).any(|w| w[0] >= w[1]) {
        return Err("block order: keys repeat or go backwards".to_owned());
    }
    Ok(())
}

pub fn check(events: &[UsageEvent], options: &RollupOptions, out: &[Rollup]) -> Result<(), String> {
    check_conservation(events.iter(), out)?;
    match options.by {
        RollupBy::Block => check_block(events, options, out),
        _ => check_keyed(events, options, out),
    }
}

pub fn describe(events: &[UsageEvent], options: &RollupOptions) -> String {
    let rows: Vec<String> = events
        .iter()
        .map(|e| {
            format!(
                "{{harness/session/model/workspace {}/{}/{}/{} at {} tokens {:?} cacheWrite1h {:?} calls {:?} cost {:?} turn {}}}",
                e.harness,
                e.session_id,
                e.model,
                e.workspace.as_deref().unwrap_or("-"),
                e.timestamp,
                [
                    e.tokens.input,
                    e.tokens.output,
                    e.tokens.cache_read,
                    e.tokens.cache_write,
                    e.tokens.reasoning
                ],
                e.tokens.cache_write1h,
                e.calls,
                e.cost_usd,
                e.turn
            )
        })
        .collect();
    format!("{options:?} over [{}]", rows.join(", "))
}
