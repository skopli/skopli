//! Bounded exhaustive check of `skopli_core::rollup`: every sequence of up to
//! `MAX_EVENTS` events drawn from the finite domain below is rolled up, and the
//! result is checked against the promises in `.agents/skills/verify-skopli-core/SKILL.md`.

use skopli_core::readers::shared::date_parse_ms;
use skopli_core::rollup::{DEFAULT_BLOCK_MS, Rollup, RollupBy, RollupOptions, rollup, rollup_key};
use skopli_core::time::to_iso_string;
use skopli_core::types::{TokenCounts, UsageEvent};

pub const MAX_EVENTS: usize = 3;
pub const KEYS: [&str; 3] = ["b", "a", "c"];
/// Instants around the hour, day and five-hour block edges, plus one that does not parse.
pub const INSTANTS: [&str; 7] = [
    "2026-03-01T22:30:00.000Z",
    "2026-03-01T22:59:59.999Z",
    "2026-03-01T23:00:00.000Z",
    "2026-03-02T00:00:00.000Z",
    "2026-03-02T03:59:59.999Z",
    "2026-03-02T04:00:00.000Z",
    "not-a-date",
];
pub const BLOCK_WIDTHS: [Option<i64>; 5] = [
    None,
    Some(0),
    Some(3_600_000),
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

pub fn event(key: &str, instant: &str, p: &Pattern) -> UsageEvent {
    let [input, output, cache_read, cache_write, reasoning] = p.tokens;
    UsageEvent {
        harness: key.to_owned(),
        timestamp: instant.to_owned(),
        session_id: key.to_owned(),
        message_id: "m".to_owned(),
        turn: p.turn,
        subagent: false,
        model: key.to_owned(),
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
        workspace: Some(key.to_owned()),
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
/// and block vary instant and shape under one key.
pub fn cases() -> Vec<(Vec<UsageEvent>, RollupOptions)> {
    let keyed: Vec<UsageEvent> = KEYS
        .iter()
        .flat_map(|k| PATTERNS.iter().map(|p| event(k, INSTANTS[0], p)))
        .collect();
    let timed: Vec<UsageEvent> = INSTANTS
        .iter()
        .flat_map(|t| TIME_PATTERNS.iter().map(|&i| event("k", t, &PATTERNS[i])))
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
        for tz in ["UTC", "America/New_York"] {
            out.push((events.clone(), opts(RollupBy::Day, tz, None)));
        }
        for width in BLOCK_WIDTHS {
            out.push((events.clone(), opts(RollupBy::Block, "UTC", width)));
        }
    }
    out
}

fn sum<T: Into<u128>>(items: impl Iterator<Item = T>) -> u128 {
    items.map(Into::into).sum()
}

fn clamped1h(e: &UsageEvent) -> u128 {
    e.tokens
        .cache_write1h
        .map_or(0, |v| v.min(e.tokens.cache_write).into())
}

fn finite_cost(e: &UsageEvent) -> Option<f64> {
    e.cost_usd.filter(|c| c.is_finite())
}

/// Promise 1: a rollup moves every counter into exactly one row and invents none.
pub fn check_conservation(events: &[UsageEvent], out: &[Rollup]) -> Result<(), String> {
    type Counter = fn(&TokenCounts) -> u64;
    let pairs: [(&str, Counter); 5] = [
        ("input", |t| t.input),
        ("output", |t| t.output),
        ("cacheRead", |t| t.cache_read),
        ("cacheWrite", |t| t.cache_write),
        ("reasoning", |t| t.reasoning),
    ];
    for (name, get) in pairs {
        let want = sum(events.iter().map(|e| get(&e.tokens)));
        let got = sum(out.iter().map(|r| get(&r.tokens)));
        if want != got {
            return Err(format!("{name}: events sum to {want}, rows sum to {got}"));
        }
    }
    let want1h = sum(events.iter().map(clamped1h));
    let got1h = sum(out.iter().map(|r| r.tokens.cache_write1h.unwrap_or(0)));
    if want1h != got1h {
        return Err(format!(
            "cacheWrite1h: clamped events sum to {want1h}, rows sum to {got1h}"
        ));
    }
    if let Some(r) = out.iter().find(|r| {
        r.tokens
            .cache_write1h
            .is_some_and(|v| v > r.tokens.cache_write)
    }) {
        return Err(format!("row {} has cacheWrite1h above cacheWrite", r.key));
    }
    if sum(out.iter().map(|r| r.events)) != events.len() as u128 {
        return Err("events: row count differs from input count".to_owned());
    }
    if sum(out.iter().map(|r| r.turns)) != events.iter().filter(|e| e.turn).count() as u128 {
        return Err("turns: rows do not count the turn flags".to_owned());
    }
    let want_calls = sum(events.iter().map(|e| e.calls.unwrap_or(1)));
    if sum(out.iter().map(|r| r.calls)) != want_calls {
        return Err(format!("calls: events sum to {want_calls}"));
    }
    let want_cost: f64 = events.iter().filter_map(finite_cost).sum();
    let got_cost: f64 = out.iter().filter_map(|r| r.cost_usd).sum();
    if want_cost != got_cost {
        return Err(format!(
            "costUsd: finite events sum to {want_cost}, rows sum to {got_cost}"
        ));
    }
    Ok(())
}

/// Promise 2: keyed rows are the distinct keys in ascending byte order, and each
/// row holds exactly the events with its key.
pub fn check_keyed(
    events: &[UsageEvent],
    options: &RollupOptions,
    out: &[Rollup],
) -> Result<(), String> {
    let mut want: Vec<String> = events.iter().map(|e| rollup_key(e, options)).collect();
    want.sort();
    want.dedup();
    let got: Vec<&str> = out.iter().map(|r| r.key.as_str()).collect();
    if want != got {
        return Err(format!("keys: want {want:?}, got {got:?}"));
    }
    for r in out {
        let mine: Vec<&UsageEvent> = events
            .iter()
            .filter(|e| rollup_key(e, options) == r.key)
            .collect();
        if r.events as usize != mine.len() {
            return Err(format!(
                "row {}: {} events, {} share the key",
                r.key,
                r.events,
                mine.len()
            ));
        }
        if r.tokens.cache_write1h.is_some() != mine.iter().any(|e| e.tokens.cache_write1h.is_some())
        {
            return Err(format!(
                "row {}: cacheWrite1h presence differs from its events",
                r.key
            ));
        }
        if r.cost_usd.is_some() != mine.iter().any(|e| finite_cost(e).is_some()) {
            return Err(format!(
                "row {}: costUsd presence differs from its events",
                r.key
            ));
        }
    }
    Ok(())
}

/// The documented block rule, written from `golden/rollup-block/README.md`: sort
/// by instant, open a block at the first event floored to the UTC hour, keep
/// events while `ms < start + width`, and send unparseable instants to one
/// trailing `invalid-date` row.
pub fn block_spec(events: &[UsageEvent], width: Option<i64>) -> Vec<(String, u64)> {
    let width = width.filter(|w| *w > 0).unwrap_or(DEFAULT_BLOCK_MS);
    let mut parsed: Vec<Option<i64>> = events.iter().map(|e| date_parse_ms(&e.timestamp)).collect();
    parsed.sort();
    let mut rows: Vec<(String, u64)> = Vec::new();
    let mut end: Option<i64> = None;
    for ms in parsed.into_iter().flatten() {
        if end.is_none_or(|end| ms >= end) {
            let start = ms - ms.rem_euclid(3_600_000);
            end = Some(start.saturating_add(width));
            rows.push((to_iso_string(start).expect("in range"), 0));
        }
        rows.last_mut().expect("open block").1 += 1;
    }
    let invalid = events
        .iter()
        .filter(|e| date_parse_ms(&e.timestamp).is_none())
        .count() as u64;
    if invalid > 0 {
        rows.push(("invalid-date".to_owned(), invalid));
    }
    rows
}

/// Promise 3: block rows match the documented rule and carry distinct,
/// ascending, hour-aligned keys.
pub fn check_block(
    events: &[UsageEvent],
    options: &RollupOptions,
    out: &[Rollup],
) -> Result<(), String> {
    let got: Vec<(String, u64)> = out.iter().map(|r| (r.key.clone(), r.events)).collect();
    let want = block_spec(events, options.block_ms);
    if want != got {
        return Err(format!("blocks: spec gives {want:?}, rollup gives {got:?}"));
    }
    let starts: Vec<i64> = out.iter().filter_map(|r| date_parse_ms(&r.key)).collect();
    if let Some(bad) = starts.iter().find(|s| s.rem_euclid(3_600_000) != 0) {
        return Err(format!("block start {bad} is not on the hour"));
    }
    if starts.windows(2).any(|w| w[0] >= w[1]) {
        return Err("block keys repeat or go backwards".to_owned());
    }
    Ok(())
}

pub fn check(events: &[UsageEvent], options: &RollupOptions) -> Result<Vec<Rollup>, String> {
    let out = rollup(events, options);
    check_conservation(events, &out)?;
    match options.by {
        RollupBy::Block => check_block(events, options, &out)?,
        _ => check_keyed(events, options, &out)?,
    }
    Ok(out)
}

pub fn describe(events: &[UsageEvent], options: &RollupOptions) -> String {
    let rows: Vec<String> = events
        .iter()
        .map(|e| {
            format!(
                "{{key {} at {} tokens {:?} cacheWrite1h {:?} calls {:?} cost {:?} turn {}}}",
                e.model,
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
