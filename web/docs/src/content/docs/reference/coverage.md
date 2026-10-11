---
title: Coverage and fixtures
description: How Skopli proves each reader is right - golden fixtures per harness, the expected outputs they pin, the registry test, and how to add or fix a reader.
---

## Golden fixtures

Every harness has a directory under `golden/` with one or more cases.
A case is a captured log (`input/`), a manifest describing it (`input-manifest.json`), and the outputs the reader must produce from it: `expected-events.json` and `expected-rollup.json`.

```text
golden/
  registry/ids.json          the 40 harness ids, in order
  claude/basic/
    input/                   a real session log, anonymized
    input-manifest.json
    expected-events.json     the UsageEvent list the reader must emit
    expected-rollup.json     the rollup of those events
```

The test suite runs every reader over every case and diffs the result against the expected files.
Any change in a reader's output, intended or not, shows up as a failing diff until the expected file is regenerated and the change reviewed.

## What the fixtures prove

- Token counters are read from the right fields and stay disjoint.
- Turn detection, `calls`, `subagent`, `workspace`, `title` and `costUsd` match the log.
- Timestamps are normalized to UTC.
- Malformed lines produce diagnostics rather than failures.

What they do not prove: that a harness never writes a field absent from its fixture, or that a log format will not change in a future release of that tool.
The [harness table](/skopli/reference/harnesses/) is derived from the fixtures for exactly this reason.

## The registry test

`golden/registry/ids.json` is the single list of harness ids.
The Rust core's conformance test, the TypeScript detect test, the Python, Go, Java, C# and Swift smoke tests and this site's harness test all assert their ids equal that file, so a harness cannot be added in one place and forgotten in another.

## Facades share the core

All nine language surfaces call the same Rust core, so a reader fix lands once.
Each facade has its own test that round-trips the golden cases through its binding, which catches serialization mistakes at the language boundary.

## Adding or fixing a reader

1. Capture a session log from the tool with any personal content removed, and place it under `golden/<id>/<case>/input/`.
2. Write or adjust the reader in `crates/skopli-core/src/readers/`.
3. Produce `expected-events.json` and `expected-rollup.json` (the Rust implementation can generate them), then review the numbers: every one should be explainable from the input.
4. If the harness is new, append its id to `golden/registry/ids.json` and its display name to the harness list.
5. Run the workspace check. CI repeats it and also asserts that the generated C header and the C# and Java bindings produce no diff.

## Schema version

Events carry a schema version, and the Go, Java, C# and Swift facades expose the loaded core's as `schemaVersion`; compare it with the version your code was written against before trusting field names.
