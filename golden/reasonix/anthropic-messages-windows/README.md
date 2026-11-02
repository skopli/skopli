# reasonix / anthropic-messages-windows

Real capture of a multi-turn session, promoted from the harness home directory with the verification skill's promotion script.

- Wire shape: anthropic-messages.
- Gateway: usage-rewriting gateway in front of a local llama-server.
- Server: llama-server b11720 (Windows x64, CPU only), 2 slots, prompt cache on.
- Model: local model.
- Cache numbers: server-reported.
- Store paths: `AppData/Roaming/reasonix/projects`.
- Redaction: user name, home directory, project path, host name and placeholder API keys replaced; path slugs renamed to match.
- Events 4, sessions 4, diagnostics 0, skipped 0.
- Totals: input 367, output 430, cache read 44789, cache write 0, reasoning 0.
- Windows root: AppData/Roaming/reasonix/projects/<slug>/sessions; telemetry books cacheMissTokens as prompt tokens, so cache creation shows as input.
- Prompt text keeps the capture rig's project path under the redacted profile (`C:/Users/user/harnesses/target/p-limit`); it is conversation text, not a store the reader consumes.
