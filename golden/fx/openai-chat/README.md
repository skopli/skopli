# fx / openai-chat

Real capture of a multi-turn session, promoted from the harness home directory with the verification skill's promotion script.

- Wire shape: openai-chat.
- Gateway: verify-skopli scripts/gateway.ts (usage rewrite, Responses pass-through).
- Server: llama-server b11720 (macOS arm64, CPU only), 2 slots, prompt cache on.
- Model: qwen2.5-1.5b-instruct-q4_k_m (served as "qwen").
- Cache numbers: none in the log; fx books the gateway's cached tokens inside input_tokens.
- Store paths: `.fx/sessions/v2`.
- Redaction: user name, home directory, project path, host name and placeholder API keys replaced; path slugs renamed to match; SQLite text columns rewritten in place and the database checkpointed and vacuumed.
- Events 4, sessions 1, diagnostics 0, skipped 0.
- Totals: input 36314, output 99, cache read 0, cache write 0, reasoning 0.
- Captured on macOS 26.6.2 (arm64) with fx 0.0.16; the layout is the same as on Ubuntu (.fx/sessions/v2/<id>/log.jsonl plus index.jsonl), so this is the harness's only case.
- One four-turn session resumed with --resume last; the session log restates cumulative usage after every turn and the reader books each restatement as the delta from the previous one, so the four events match the four gateway requests.
- fx counts the whole prompt as input_tokens and leaves cache_read_tokens null on a custom OpenAI-compatible connection, so the gateway's cached_tokens are inside input here.
