---
title: Supported harnesses
description: The 40 harnesses Skopli reads, their ids, and which `UsageEvent` fields each one's log fills in.
---

## Detected but not read

`detectHarnesses` also reports Cursor, aider, Crush and GitLab Duo CLI when their state directories exist.
Their local files carry no reliable token counts, so Skopli lists them under `unsupported` and reads nothing from them.
If one of them starts logging usage, a reader can be added; see [Coverage and fixtures](/skopli/reference/coverage/).

## Reading the field columns

Each column marks a `UsageEvent` field that the harness's golden fixture fills in.

- `costUsd`: the log itself records a cost. Still worth pricing: logged costs use the rates the vendor saw at the time.
- `workspace`: the project directory the session ran in, enabling `rollup({ by: "workspace" })`.
- `title`: a session title, when the harness stores one.
- `calls`: the record aggregates several model calls and reports the count.
- `subagent`: the fixture contains records with `subagent: true` (the field itself is always present), so `subagents: "exclude"` has an effect.

A blank cell means the fixture does not contain the field.
It does not prove the harness never writes it.

## Counts

Every harness except Kiro CLI reports token counts in its log.
Kiro CLI does not, so its reader estimates from message text; the "Counts" column marks it as estimated.
