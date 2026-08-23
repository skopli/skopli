#!/usr/bin/env python3
"""Deterministically (re)build the Cherry Studio golden SQLite fixture.

Cherry Studio stores usage in a single WAL-mode SQLite database
(cherrystudio.sqlite) under its Electron userData/Data directory. This script
writes a small, real-shaped ai_usage_record table covering the row kinds the
reader must handle: a per-invocation row with cache splits and reasoning, a
legacy-aggregate backfill row, and an all-zero row that the reader drops. The
resulting binary is committed as the golden input; run this script to regenerate
it if the schema or the expected rows change.
"""

import os
import sqlite3

HERE = os.path.dirname(os.path.abspath(__file__))
DB = os.path.join(
    HERE, "input", ".config", "CherryStudio", "Data", "cherrystudio.sqlite"
)


def main() -> None:
    for suffix in ("", "-wal", "-shm"):
        try:
            os.remove(DB + suffix)
        except FileNotFoundError:
            pass
    conn = sqlite3.connect(DB)
    try:
        conn.executescript(
            """
            CREATE TABLE ai_usage_record (
                id TEXT PRIMARY KEY,
                input_tokens INTEGER,
                output_tokens INTEGER,
                total_tokens INTEGER,
                reasoning_tokens INTEGER,
                no_cache_tokens INTEGER,
                cache_read_tokens INTEGER,
                cache_write_tokens INTEGER,
                model_id TEXT,
                model_name TEXT,
                provider_id TEXT,
                provider_name TEXT,
                created_at INTEGER,
                message_kind TEXT,
                message_id TEXT,
                record_kind TEXT
            );
            -- an invocation row with cache splits + reasoning
            INSERT INTO ai_usage_record VALUES (
                'inv1', 100, 40, 140, 10, 5, 20, 8,
                'claude-sonnet-4-5', 'Claude Sonnet 4.5', 'anthropic', 'Anthropic',
                1754042400000, 'chat', 'msg-inv1', 'invocation'
            );
            -- a legacy-aggregate row (pre-migration rollup) that must survive
            INSERT INTO ai_usage_record VALUES (
                'agg1', 1000, 500, 1500, 0, 0, 0, 0,
                'gpt-4o', 'GPT-4o', 'openai', 'OpenAI',
                1750000000000, 'chat', 'msg-agg1', 'legacy-aggregate'
            );
            -- a zero-token row that is dropped
            INSERT INTO ai_usage_record VALUES (
                'zero1', 0, 0, 0, 0, 0, 0, 0,
                'gpt-4o', 'GPT-4o', 'openai', 'OpenAI',
                1750000000000, 'chat', 'msg-zero', 'invocation'
            );
            """
        )
        conn.commit()
    finally:
        conn.close()


if __name__ == "__main__":
    main()
