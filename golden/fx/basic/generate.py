#!/usr/bin/env python3
"""Deterministically (re)build the fx.sh golden usage fixture.

fx (vercel-labs/fx) is a native coding agent that persists per-session usage at
~/.fx/sessions/<session_id>/usage-v2.json. The profile root constant is ".fx"
joined to the OS home, with no environment override, so the sessions directory
resolves from home only. Each sidecar is a {schema_version, session_id,
snapshot} envelope written with exactly three keys; schema_version must equal 1.
The snapshot carries authoritative provider token and cost aggregates plus a
models[] array of per-model aggregates with provider-qualified ids
(provider/model). Counts are real AI Gateway billing figures, not estimates.

The sidecars emit the COMPLETE producer shape confirmed in E11
(.scratch/expansion/receipts/E11-fx-identity.md), including the fields the reader
does not consume: the snapshot booleans (api_duration_complete,
wall_duration_complete, code_complete), the invocation cursors (next_sequence,
settled_through_sequence), the durations (api_duration_ms, wall_duration_ms),
billable_web_search_calls, lines_added, lines_removed, and the pending /
publication_backlog arrays; and per model the first_sequence and
billable_web_search_calls fields. The reader validates only the subset it
consumes (see fx.rs), so these extra fields are UNREAD: they exist in the fixture
purely so that fx producer-schema drift stays visible in review, and adding or
removing them must not change expected-events.json or expected-rollup.json.
incidents is not unread: the reader inspects it permissively and emits an
informational warning when it is a non-empty array; the fixture keeps it empty
so no warning is expected.

This fixture covers the row kinds the reader must handle: a complete session
with two models (one event per models[] entry, provider-qualified ids stored
as-is); a legacy session whose reasoning_tokens and request_count are null (a
zero reasoning count and an absent call count); a pending-billing session that
is still read (usage that exists is usage); and a malformed sidecar whose
schema_version is not 1 (skipped with a {path}:usage-v2:{ordinal} diagnostic
over the sorted enumeration order). The event timestamp derives from the session
id's leading millisecond prefix ({ms}-{ns}-{16hex}).

The complete session's snapshot-level totals are deliberately NOT the sum of its
models[] rows: the reader emits one event per model aggregate and never reads the
snapshot totals, so the fixture keeps them distinct to document that no
sum-of-rows invariant holds. Each event's message_id is {session_id}:{model},
keyed on the provider-qualified model name (unique per aggregate, enforced by the
reader) rather than the array position. Run this script to regenerate the fixture
if the schema or the expected rows change.
"""

import json
import os

HERE = os.path.dirname(os.path.abspath(__file__))
SESSIONS = os.path.join(HERE, "input", ".fx", "sessions")

COMPLETE_ID = "1770000000000-1770000000000000000-a1b2c3d4e5f60718"
LEGACY_ID = "1770000100000-1770000000000000001-b1b2c3d4e5f60718"
PENDING_ID = "1770000200000-1770000000000000002-c1b2c3d4e5f60718"
MALFORMED_ID = "1770000300000-1770000000000000003-d1b2c3d4e5f60718"


def model_aggregate(
    model,
    first_sequence,
    total_cost,
    input_tokens,
    output_tokens,
    cache_read_tokens,
    cache_write_tokens,
    reasoning_tokens,
    request_count,
    billable_web_search_calls=0,
):
    """A full E11 ModelAggregate. first_sequence and billable_web_search_calls
    are present in the producer shape but are not read by the reader."""
    return {
        "model": model,
        "first_sequence": first_sequence,
        "total_cost": total_cost,
        "input_tokens": input_tokens,
        "output_tokens": output_tokens,
        "cache_read_tokens": cache_read_tokens,
        "cache_write_tokens": cache_write_tokens,
        "reasoning_tokens": reasoning_tokens,
        "request_count": request_count,
        "billable_web_search_calls": billable_web_search_calls,
    }


def snapshot(
    billing,
    total_cost,
    input_tokens,
    output_tokens,
    cache_read_tokens,
    cache_write_tokens,
    reasoning_tokens,
    request_count,
    models,
    *,
    next_sequence=0,
    settled_through_sequence=0,
    billable_web_search_calls=0,
    lines_added=0,
    lines_removed=0,
    pending=None,
    publication_backlog=None,
    incidents=None,
):
    """A full E11 Snapshot. Every field beyond schema_version, billing, the token
    counters, reasoning_tokens, request_count, and models is present as producer
    shape only; the reader does not consume it, except incidents, which the
    reader inspects permissively to emit an informational warning when it is a
    non-empty array."""
    return {
        "schema_version": 1,
        "billing": billing,
        "api_duration_complete": True,
        "wall_duration_complete": True,
        "code_complete": True,
        "next_sequence": next_sequence,
        "settled_through_sequence": settled_through_sequence,
        "api_duration_ms": 0,
        "wall_duration_ms": 0,
        "total_cost": total_cost,
        "input_tokens": input_tokens,
        "output_tokens": output_tokens,
        "cache_read_tokens": cache_read_tokens,
        "cache_write_tokens": cache_write_tokens,
        "reasoning_tokens": reasoning_tokens,
        "request_count": request_count,
        "billable_web_search_calls": billable_web_search_calls,
        "lines_added": lines_added,
        "lines_removed": lines_removed,
        "models": models,
        "pending": pending if pending is not None else [],
        "publication_backlog": publication_backlog
        if publication_backlog is not None
        else [],
        "incidents": incidents if incidents is not None else [],
    }


def write_sidecar(session_id, schema_version, snapshot_obj):
    session_dir = os.path.join(SESSIONS, session_id)
    os.makedirs(session_dir, exist_ok=True)
    envelope = {
        "schema_version": schema_version,
        "session_id": session_id,
        "snapshot": snapshot_obj,
    }
    with open(os.path.join(session_dir, "usage-v2.json"), "w", encoding="utf-8") as f:
        json.dump(envelope, f, separators=(",", ":"))


def main() -> None:
    write_sidecar(
        COMPLETE_ID,
        1,
        snapshot(
            billing="complete",
            total_cost=0.99,
            input_tokens=999,
            output_tokens=999,
            cache_read_tokens=999,
            cache_write_tokens=999,
            reasoning_tokens=999,
            request_count=99,
            next_sequence=3,
            settled_through_sequence=2,
            billable_web_search_calls=2,
            lines_added=42,
            lines_removed=7,
            models=[
                model_aggregate(
                    "openai/gpt-5.4",
                    first_sequence=1,
                    total_cost=0.03,
                    input_tokens=20,
                    output_tokens=8,
                    cache_read_tokens=4,
                    cache_write_tokens=2,
                    reasoning_tokens=4,
                    request_count=1,
                    billable_web_search_calls=1,
                ),
                model_aggregate(
                    "anthropic/claude-sonnet-4-5",
                    first_sequence=2,
                    total_cost=0.02,
                    input_tokens=10,
                    output_tokens=4,
                    cache_read_tokens=2,
                    cache_write_tokens=1,
                    reasoning_tokens=0,
                    request_count=1,
                    billable_web_search_calls=0,
                ),
            ],
        ),
    )

    write_sidecar(
        LEGACY_ID,
        1,
        snapshot(
            billing="legacy",
            total_cost=0.01,
            input_tokens=10,
            output_tokens=4,
            cache_read_tokens=0,
            cache_write_tokens=0,
            reasoning_tokens=None,
            request_count=None,
            next_sequence=1,
            settled_through_sequence=1,
            models=[
                model_aggregate(
                    "openai/gpt-5.4",
                    first_sequence=1,
                    total_cost=0.01,
                    input_tokens=10,
                    output_tokens=4,
                    cache_read_tokens=0,
                    cache_write_tokens=0,
                    reasoning_tokens=None,
                    request_count=None,
                )
            ],
        ),
    )

    write_sidecar(
        PENDING_ID,
        1,
        snapshot(
            billing="pending",
            total_cost=0.02,
            input_tokens=15,
            output_tokens=5,
            cache_read_tokens=0,
            cache_write_tokens=0,
            reasoning_tokens=0,
            request_count=1,
            next_sequence=2,
            settled_through_sequence=0,
            models=[
                model_aggregate(
                    "anthropic/claude-sonnet-4-5",
                    first_sequence=1,
                    total_cost=0.02,
                    input_tokens=15,
                    output_tokens=5,
                    cache_read_tokens=0,
                    cache_write_tokens=0,
                    reasoning_tokens=0,
                    request_count=1,
                )
            ],
            pending=[
                {
                    "id": "gen-1",
                    "sequence": 1,
                    "origin": "https://ai-gateway.vercel.sh",
                }
            ],
        ),
    )

    write_sidecar(
        MALFORMED_ID,
        2,
        {
            "schema_version": 2,
            "billing": "complete",
            "models": [],
        },
    )


if __name__ == "__main__":
    main()
