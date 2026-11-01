# goose / anthropic-messages-windows

Real capture of a multi-turn session, promoted from the harness home directory with the verification skill's promotion script.

- Wire shape: anthropic-messages-windows.
- Gateway: usage-rewriting gateway in front of a local llama-server.
- Server: llama-server b11720 (Windows x64, CPU only), 2 slots, prompt cache on.
- Model: local model.
- Cache numbers: server-reported.
- Store paths: `AppData/Roaming/Block/goose/data/sessions/sessions.db`.
- Redaction: user name, home directory, project path, host name and placeholder API keys replaced; path slugs renamed to match; SQLite text columns rewritten in place and the database checkpointed and vacuumed.
- Events 4, sessions 4, diagnostics 0, skipped 0.
- Totals: input 0, output 208, cache read 36652, cache write 15738, reasoning 0.
- Windows root: AppData/Roaming/Block/goose/data/sessions/sessions.db; the sessions table carries cache_read_tokens and cache_write_tokens inside a cache-inclusive input_tokens, the reader splits them.
