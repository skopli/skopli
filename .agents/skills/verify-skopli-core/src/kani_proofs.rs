//! Kani proofs of the pure integer kernels in `skopli_core::rollup`. Run one at
//! a time from the repository root:
//! `timeout 600 cargo kani --manifest-path .agents/skills/verify-skopli-core/Cargo.toml --harness <name>`

use skopli_core::rollup::{add_tokens, block_end, block_start};
use skopli_core::types::TokenCounts;

use crate::HOUR_MS;

/// V8's time value range; every instant the core formats lies inside it.
const MAX_TIME_MS: i64 = 8_640_000_000_000_000;

fn any_counts() -> TokenCounts {
    TokenCounts {
        input: kani::any(),
        output: kani::any(),
        cache_read: kani::any(),
        cache_write: kani::any(),
        cache_write1h: kani::any(),
        reasoning: kani::any(),
    }
}

/// Every counter is the saturating sum, the 1h split is clamped to its own
/// event's writes first, and a row whose split never exceeded its writes keeps
/// that invariant.
#[kani::proof]
fn add_tokens_saturates_and_keeps_the_split_under_the_writes() {
    let before = any_counts();
    let event = any_counts();
    let mut sum = before.clone();
    add_tokens(&mut sum, &event);
    assert_eq!(sum.input, before.input.saturating_add(event.input));
    assert_eq!(sum.output, before.output.saturating_add(event.output));
    assert_eq!(
        sum.cache_read,
        before.cache_read.saturating_add(event.cache_read)
    );
    assert_eq!(
        sum.cache_write,
        before.cache_write.saturating_add(event.cache_write)
    );
    assert_eq!(
        sum.reasoning,
        before.reasoning.saturating_add(event.reasoning)
    );
    match event.cache_write1h {
        None => assert_eq!(sum.cache_write1h, before.cache_write1h),
        Some(split) => assert_eq!(
            sum.cache_write1h,
            Some(
                before
                    .cache_write1h
                    .unwrap_or(0)
                    .saturating_add(split.min(event.cache_write))
            )
        ),
    }
    if before
        .cache_write1h
        .is_none_or(|split| split <= before.cache_write)
    {
        assert!(
            sum.cache_write1h
                .is_none_or(|split| split <= sum.cache_write)
        );
    }
}

fn any_instant() -> i64 {
    let ms: i64 = kani::any();
    kani::assume((-MAX_TIME_MS..=MAX_TIME_MS).contains(&ms));
    ms
}

fn any_width() -> i64 {
    let width: i64 = kani::any();
    kani::assume(width > 0);
    width
}

/// Milliseconds past the start of the local hour, as any zone's wall clock
/// reports it: a value in `[0, HOUR_MS)`, otherwise unconstrained, so the proofs
/// hold for every zone rather than for UTC alone.
fn any_within_hour() -> i64 {
    let within: i64 = kani::any();
    kani::assume((0..HOUR_MS).contains(&within));
    within
}

/// The block start never exceeds the event, lies less than one grain before it,
/// never leaves the local hour, equals the hour start for a width of an hour or
/// more, and the event falls inside `[start, end)`. Neither kernel panics for
/// any in-range instant and positive width.
#[kani::proof]
fn block_start_floors_to_the_grain_and_contains_the_event() {
    let ms = any_instant();
    let width = any_width();
    let within = any_within_hour();
    let start = block_start(ms, within, width);
    let end = block_end(start, width);
    let grain = width.min(HOUR_MS);
    let hour_start = ms - within;
    assert!(start <= ms);
    assert!(ms - start < grain);
    assert!(start >= hour_start);
    assert!(ms < end);
    if width >= HOUR_MS {
        assert_eq!(start, hour_start);
    }
}

/// An event that falls past the open block's end opens a block with a strictly
/// later start, so block keys are distinct and ascending.
#[kani::proof]
fn later_block_starts_strictly_after_the_open_one() {
    let first = any_instant();
    let next = any_instant();
    let width = any_width();
    let start = block_start(first, any_within_hour(), width);
    kani::assume(next >= block_end(start, width));
    let next_start = block_start(next, any_within_hour(), width);
    assert!(next_start > start);
}
