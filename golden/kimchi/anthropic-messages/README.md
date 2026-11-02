# kimchi / anthropic-messages

Real capture of a multi-turn session, promoted from the harness home directory with the verification skill's promotion script.

- Wire shape: anthropic-messages.
- Gateway: usage-rewriting gateway in front of a local llama-server.
- Server: llama-server b11720 (Windows x64, CPU only), 2 slots, prompt cache on.
- Model: local model.
- Cache numbers: server-reported.
- Store paths: `.config/kimchi/harness/sessions`.
- Redaction: user name, home directory, project path, host name and placeholder API keys replaced; path slugs renamed to match.
- Events 8, sessions 8, diagnostics 0, skipped 0.
- Totals: input 0, output 157, cache read 60032, cache write 10046, reasoning 0.
- Every record also carries `usage.cacheWrite1h` (0 here), which the reader surfaces as `cacheWrite1h`.
