# omp / openai-chat

Real capture of a multi-turn session, promoted from the harness home directory with the verification skill's promotion script.

- Wire shape: openai-chat.
- Gateway: verify-skopli scripts/gateway.ts (usage rewrite, Responses pass-through).
- Server: llama-server b11720, 4 slots, prompt cache on.
- Model: qwen2.5-1.5b-instruct-q4_k_m (served as "qwen").
- Cache numbers: server-reported prompt_tokens_details.cached_tokens from the gateway (timings.cache_n).
- Store paths: `.omp/agent/sessions`.
- Redaction: user name, home directory, project path, host name and placeholder API keys replaced; path slugs renamed to match; SQLite text columns rewritten in place and the database checkpointed and vacuumed.
- Events 4, sessions 1, diagnostics 0, skipped 0.
- Totals: input 7590, output 113, cache read 22747, cache write 0, reasoning 0.
