# reasonix / openai-chat

Real capture of a multi-turn session, promoted from the harness home directory with the verification skill's promotion script.

- Wire shape: openai-chat.
- Gateway: verify-skopli scripts/gateway.ts (usage rewrite, Responses pass-through).
- Server: llama-server b11720 (Linux x64, CPU only), 4 slots, prompt cache on.
- Model: qwen2.5-1.5b-instruct-q4_k_m (served as "qwen").
- Cache numbers: server-reported prompt_tokens_details.cached_tokens from the gateway (timings.cache_n), booked by the harness as cache_hit.
- Store paths: `.reasonix/projects`, `.reasonix/stats`.
- Redaction: user name, home directory, project path, host name and placeholder API keys replaced; path slugs renamed to match.
- Events 4, sessions 1, diagnostics 0, skipped 0.
- Totals: input 8279, output 162, cache read 20873, cache write 0, reasoning 0.
- Reasonix 2.44.0 (reasonix run --model qwen, then reasonix run -c --model qwen three times); the per-request usage store is .reasonix/stats/<date>.jsonl, one row per request plus a {turn:true} marker per turn, and the reader emits one event per row keyed by the file date.
- The .jsonl.telemetry.json sidecar beside the transcript holds only the last request's counts (its promptTokens equals the final stats row's prompt), so a home with a stats ledger is read from the ledger alone; the transcript, events, ckpt and recovery files carry no usage.
- Totals equal the gateway log: cache_miss is the uncached prompt, cache_hit the cached prefix.
