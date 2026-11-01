# codex / openai-responses

Real capture of a multi-turn session, promoted from the harness home directory with the verification skill's promotion script.

- Wire shape: openai-responses.
- Gateway: usage-rewriting gateway in front of a local llama-server.
- Server: llama-server b11720 (Windows x64, CPU only), 2 slots, prompt cache on.
- Model: local model.
- Cache numbers: server-reported.
- Store paths: `.codex/sessions`.
- Redaction: user name, home directory, project path, host name and placeholder API keys replaced; path slugs renamed to match; SQLite text columns rewritten in place and the database checkpointed and vacuumed.
- Events 7, sessions 7, diagnostics 0, skipped 0.
- Totals: input 11519, output 291, cache read 68689, cache write 0, reasoning 0.
- The home holds eight rollouts: four from a first attempt that exited before the recorded run and four from the recorded run; seven carry a token_count event.
