#!/usr/bin/env python3
"""Deterministically (re)build the DeepSeek Harness SQLite golden fixture.

DeepSeek Harness ships an optional SQLite persistence backend (schema 17,
application id 0x44534850). It stores the same SessionEvent vocabulary the JSONL
backend uses: a sessions table for per-session metadata and an events table
whose data column holds each event's JSON (plaintext below 4096 bytes). This
case pairs a JSONL session with a SQLite database that repeats one of its events
so the reader's session_id:seq dedupe (JSONL wins) is exercised alongside the
SQLite-only rows. One SQLite row stores its data as a Zstandard blob (the
backend compresses data at or above 4096 bytes) so the reader's blob branch is
exercised end to end. Run this script to regenerate the binary if the schema or
the expected rows change.
"""

import json
import os
import sqlite3

HERE = os.path.dirname(os.path.abspath(__file__))
DB = os.path.join(HERE, "input", "sessions", "db", "sessions.db")

APPLICATION_ID = 0x44534850
SCHEMA_VERSION = 17


def event_data(model, usage):
    return json.dumps(
        {"message": {"source": {"model": model}}, "usage": usage},
        separators=(",", ":"),
    )


def zstd_raw_frame(text):
    payload = text.encode("utf-8")
    if len(payload) >= 256:
        raise ValueError("this helper only frames payloads below 256 bytes")
    header = bytes([0x28, 0xB5, 0x2F, 0xFD, 0x20, len(payload)])
    block_header = (len(payload) << 3) | 1
    block = bytes(
        [block_header & 0xFF, (block_header >> 8) & 0xFF, (block_header >> 16) & 0xFF]
    )
    return header + block + payload


def main():
    os.makedirs(os.path.dirname(DB), exist_ok=True)
    for suffix in ("", "-wal", "-shm"):
        try:
            os.remove(DB + suffix)
        except FileNotFoundError:
            pass
    conn = sqlite3.connect(DB)
    try:
        conn.executescript(
            f"""
            PRAGMA application_id = {APPLICATION_ID};
            PRAGMA user_version = {SCHEMA_VERSION};
            CREATE TABLE persistence_state (
                singleton INTEGER PRIMARY KEY CHECK (singleton = 1),
                store_id  TEXT NOT NULL
            ) STRICT;
            CREATE TABLE sessions (
                id               TEXT PRIMARY KEY,
                version          INTEGER NOT NULL,
                created_at       INTEGER NOT NULL,
                cwd              TEXT,
                parent_session   TEXT,
                seed_length      INTEGER,
                origin           TEXT,
                delegation_depth INTEGER,
                agent_preset     TEXT,
                incarnation      TEXT NOT NULL,
                revision         INTEGER NOT NULL
            ) STRICT;
            CREATE TABLE events (
                session_id        TEXT NOT NULL REFERENCES sessions(id) ON DELETE CASCADE,
                seq               INTEGER NOT NULL,
                type              TEXT NOT NULL,
                time              INTEGER NOT NULL,
                data              ANY NOT NULL,
                source_event_seqs ANY,
                surface_op        TEXT,
                ignorable         INTEGER CHECK (ignorable IS NULL OR ignorable IN (0, 1)),
                PRIMARY KEY (session_id, seq)
            ) STRICT;
            """
        )
        conn.execute(
            "INSERT INTO persistence_state VALUES (1, ?)",
            ("00000000-0000-4000-8000-000000000000",),
        )
        conn.executemany(
            "INSERT INTO sessions VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
            [
                (
                    "sess-main", 1, 1754042400000, "/home/u/proj",
                    None, None, None, 0, None, "i-main", 3,
                ),
                (
                    "sess-db", 1, 1754046000000, "/home/u/proj",
                    "sess-main", None, "subagent", 1, None, "i-db", 1,
                ),
            ],
        )
        conn.executemany(
            "INSERT INTO events VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
            [
                (
                    "sess-main", 2, "assistant/message", 1754042402000,
                    event_data(
                        "deepseek-chat",
                        {
                            "inputTokens": 100, "outputTokens": 50,
                            "cacheReadTokens": 30, "cacheWriteTokens": 20,
                            "reasoningTokens": 7,
                        },
                    ),
                    None, None, None,
                ),
                (
                    "sess-main", 5, "assistant/message", 1754042405000,
                    event_data(
                        "deepseek-chat",
                        {"inputTokens": 11, "outputTokens": 22},
                    ),
                    None, None, None,
                ),
                (
                    "sess-db", 1, "assistant/message", 1754046001000,
                    event_data(
                        "deepseek-reasoner",
                        {
                            "inputTokens": 200, "outputTokens": 80,
                            "reasoningTokens": 30,
                        },
                    ),
                    None, None, None,
                ),
                (
                    "sess-db", 2, "assistant/message", 1754046002000,
                    zstd_raw_frame(
                        event_data(
                            "deepseek-reasoner",
                            {"inputTokens": 40, "outputTokens": 15},
                        )
                    ),
                    None, None, None,
                ),
            ],
        )
        conn.commit()
    finally:
        conn.close()


if __name__ == "__main__":
    main()
