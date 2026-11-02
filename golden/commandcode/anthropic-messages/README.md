# commandcode / anthropic-messages

Real capture of a multi-turn session, promoted from the harness home directory with the verification skill's promotion script.

- Wire shape: anthropic-messages.
- Gateway: verify-skopli scripts/gateway.ts (usage rewrite, Responses pass-through).
- Server: llama-server b11720 (Linux x64, CPU only), 4 slots, prompt cache on.
- Model: qwen2.5-1.5b-instruct-q4_k_m (served as "qwen" through the anthropic-messages provider kind).
- Cache numbers: server-reported cache_read_input_tokens from the gateway (timings.cache_n).
- Store paths: `.commandcode/projects`.
- Redaction: user name, home directory, project path, host name and placeholder API keys replaced; path slugs renamed to match.
- Events 5, sessions 1, diagnostics 0, skipped 0.
- Totals: input 7801, output 209, cache read 66007, cache write 0, reasoning 0.
- Command Code 1.84.0 (--local-only --skip-onboarding --yolo -p, one session continued with -c); one <session-uuid>.jsonl per session under projects/<cwd-slug>/, the .checkpoints.jsonl and .meta.json beside it carry no usage.
- On this wire usage.inputTokens is the uncached input plus the cached prefix and usage.cacheReadTokens the cached part, the same cache-inclusive convention as the openai-chat case; five requests (turn one read the file first), five events, totals equal to the gateway log.
