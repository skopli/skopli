#!/usr/bin/env python3
"""Deterministically (re)build the Kiro CLI golden SQLite fixture.

Kiro CLI (the backward-compatible continuation of aws/amazon-q-developer-cli)
persists chat to a SQLite database at ~/.kiro/data.sqlite3, keyed per project
directory in a conversations (key TEXT, value TEXT) kv table. Each value is a
serialized ConversationState whose history array holds {user, assistant} turns
with the full prompt and response text; no provider token counts are persisted,
so the reader estimates tokens with the ancestor's algorithm: the UTF-8 byte
length of the text divided by four, then rounded to the nearest ten (input from
the prompt, output from the response). Turns under twenty bytes round to zero
and are dropped.

This fixture covers the row kinds the reader must handle and exercises distinct
rounding buckets: a conversation with two completed turns whose prompts and
responses round to 10, 20, 20 and 30; a second conversation that carries its
model under model_info.model_id, holds a multibyte (accented) turn to prove the
estimate counts UTF-8 bytes rather than characters, and a pending next_message
that must not be emitted; and a malformed row whose value is not valid JSON
(skipped with a table-scoped one-based ORDER BY key ordinal diagnostic,
{db}:{table}:{ordinal}). Run this script to regenerate the binary if the schema
or the expected rows change.
"""

import json
import os
import sqlite3

HERE = os.path.dirname(os.path.abspath(__file__))
DB = os.path.join(HERE, "input", ".kiro", "data.sqlite3")


def turn(prompt, response, timestamp):
    user = {"content": {"Prompt": {"prompt": prompt}}, "timestamp": timestamp}
    assistant = {"Response": {"message_id": None, "content": response}}
    return {"user": user, "assistant": assistant}


def main() -> None:
    os.makedirs(os.path.dirname(DB), exist_ok=True)
    for suffix in ("", "-wal", "-shm"):
        try:
            os.remove(DB + suffix)
        except FileNotFoundError:
            pass

    proj = json.dumps(
        {
            "model": "claude-sonnet-4-5",
            "history": [
                turn(
                    "how are you doing today my dear old friend",
                    "the rust programming language is a fast and safe systems tool",
                    "2026-08-01T10:00:00Z",
                ),
                turn(
                    "the rust programming language is a fast and safe systems tool",
                    "rust gives you memory safety without a garbage collector and it is really quite pleasant to write code in every day",
                    "2026-08-01T10:05:00Z",
                ),
            ],
        },
        separators=(",", ":"),
    )

    other = json.dumps(
        {
            "model_info": {"model_id": "amazon-q-default"},
            "next_message": {
                "content": {"Prompt": {"prompt": "pending unsent prompt not emitted"}},
                "timestamp": None,
            },
            "history": [
                turn(
                    "un café très chaud avec des croissants frais ce matin",
                    "the capital city of france is called paris today",
                    "2026-08-02T09:30:00Z",
                ),
            ],
        },
        separators=(",", ":"),
    )

    conn = sqlite3.connect(DB)
    try:
        conn.executescript(
            """
            CREATE TABLE conversations (
                key   TEXT PRIMARY KEY,
                value TEXT
            );
            """
        )
        conn.executemany(
            "INSERT INTO conversations (key, value) VALUES (?, ?)",
            [
                ("/home/u/malformed", "this is not valid json at all"),
                ("/home/u/other", other),
                ("/home/u/proj", proj),
            ],
        )
        conn.commit()
    finally:
        conn.close()


if __name__ == "__main__":
    main()
