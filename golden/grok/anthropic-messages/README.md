# grok / anthropic-messages

Real capture of a multi-turn session, promoted from the harness home directory with the verification skill's promotion script.

- Wire shape: anthropic-messages.
- Gateway: usage-rewriting gateway in front of a local llama-server.
- Server: llama-server b11720 (Windows x64, CPU only), 2 slots, prompt cache on.
- Model: local model.
- Cache numbers: server-reported.
- Store paths: `.grok/sessions`.
- Redaction: user name, home directory, project path, host name and placeholder API keys replaced; path slugs renamed to match; SQLite text columns rewritten in place and the database checkpointed and vacuumed.
- Events 4, sessions 4, diagnostics 0, skipped 0.
- Totals: input 13888, output 282, cache read 22965, cache write 12583, reasoning 0.
- Grok books cache_creation_input_tokens as input and records the first turn, cut at max_tokens, as a prompt estimate with no output; three title requests have no row.
