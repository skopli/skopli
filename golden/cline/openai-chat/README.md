# cline / openai-chat

Real capture of a multi-turn session, promoted from the harness home directory with the verification skill's promotion script.

- Wire shape: openai-chat.
- Gateway: usage-rewriting gateway in front of a local llama-server.
- Server: llama-server b11720 (Windows x64, CPU only), 2 slots, prompt cache on.
- Model: local model.
- Cache numbers: server-reported.
- Store paths: `.cline/data/sessions`, `.cline/data/db/sessions.db`.
- Redaction: user name, home directory, project path, host name and placeholder API keys replaced; path slugs renamed to match; SQLite text columns rewritten in place and the database checkpointed and vacuumed.
- Events 228, sessions 1, diagnostics 0, skipped 0.
- Totals: input 15314, output 4338, cache read 2546701, cache write 0, reasoning 0.
- The 1.5B model looped on tool calls for the whole run, so one session holds 228 request rows; the gateway logged 229 requests.
- The reader reads the per-session JSON under .cline/data/sessions; sessions.db beside it is the CLI's own index and is kept for layout fidelity.
