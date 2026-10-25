# omp / anthropic-messages

Real capture of a multi-turn session, promoted from the harness home directory with the verification skill's promotion script.

- Wire shape: anthropic-messages.
- Gateway: verify-skopli scripts/gateway.ts (usage rewrite, Responses pass-through).
- Server: llama-server b11720, 4 slots, prompt cache on.
- Model: qwen2.5-1.5b-instruct-q4_k_m (served as "qwen").
- Cache numbers: server-reported cache_read_input_tokens; cache_creation_input_tokens is gateway-derived (uncached input booked as creation when the request carried cache_control).
- Store paths: `.omp/agent/sessions`.
- Redaction: user name, home directory, project path, host name and placeholder API keys replaced; path slugs renamed to match; SQLite text columns rewritten in place and the database checkpointed and vacuumed.
- Events 4, sessions 1, diagnostics 0, skipped 0.
- Totals: input 0, output 195, cache read 23093, cache write 7646, reasoning 0.
