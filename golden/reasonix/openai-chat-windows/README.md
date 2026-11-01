# reasonix / openai-chat-windows

Real capture of a multi-turn session, promoted from the harness home directory with the verification skill's promotion script.

- Wire shape: openai-chat-windows.
- Gateway: usage-rewriting gateway in front of a local llama-server.
- Server: llama-server b11720 (Windows x64, CPU only), 2 slots, prompt cache on.
- Model: local model.
- Cache numbers: server-reported.
- Store paths: `AppData/Roaming/reasonix/projects`.
- Redaction: user name, home directory, project path, host name and placeholder API keys replaced; path slugs renamed to match; SQLite text columns rewritten in place and the database checkpointed and vacuumed.
- Events 4, sessions 4, diagnostics 0, skipped 0.
- Totals: input 7575, output 1150, cache read 22363, cache write 0, reasoning 0.
- Windows root: AppData/Roaming/reasonix/projects/<slug>/sessions (Linux writes .reasonix/projects); usage lives in <session>.jsonl.telemetry.json with <session>.jsonl.meta for the model and timestamp, no .acp.json.
