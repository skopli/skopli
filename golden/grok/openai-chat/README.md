# grok / openai-chat

Real capture of a multi-turn session, promoted from the harness home directory with the verification skill's promotion script.

- Wire shape: openai-chat.
- Gateway: usage-rewriting gateway in front of a local llama-server.
- Server: llama-server b11720 (Windows x64, CPU only), 2 slots, prompt cache on.
- Model: local model.
- Cache numbers: server-reported.
- Store paths: `.grok/sessions`.
- Redaction: user name, home directory, project path, host name and placeholder API keys replaced; path slugs renamed to match; SQLite text columns rewritten in place and the database checkpointed and vacuumed.
- Events 4, sessions 4, diagnostics 0, skipped 0.
- Totals: input 24427, output 204, cache read 22964, cache write 0, reasoning 0.
- Grok records the four main requests only; three title requests of about 300 prompt tokens each reached the gateway and have no row.
- Prompt and system text keep the capture rig's project and temp paths under the redacted profile (`C:/Users/user/harnesses/target/p-limit`, `C:/Users/user/AppData/Local/Temp`); they are conversation text, not stores the reader consumes.
