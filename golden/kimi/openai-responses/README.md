# kimi / openai-responses

Real capture of a multi-turn session, promoted from the harness home directory with the verification skill's promotion script.

- Wire shape: openai-responses.
- Gateway: usage-rewriting gateway in front of a local llama-server.
- Server: llama-server b11720 (Windows x64, CPU only), 2 slots, prompt cache on.
- Model: local model.
- Cache numbers: server-reported.
- Store paths: `.kimi-code/sessions`.
- Redaction: user name, home directory, project path, host name and placeholder API keys replaced; path slugs renamed to match; SQLite text columns rewritten in place and the database checkpointed and vacuumed.
- Events 4, sessions 4, diagnostics 0, skipped 0.
- Totals: input 700, output 1271, cache read 54494, cache write 0, reasoning 0.
