# hermes / openai-chat

Real capture of a multi-turn session, promoted from the harness home directory with the verification skill's promotion script.

- Wire shape: openai-chat.
- Gateway: usage-rewriting gateway in front of a local llama-server.
- Server: llama-server b11720 (Windows x64, CPU only), 2 slots, prompt cache on.
- Model: local model.
- Cache numbers: server-reported.
- Store paths: `.hermes/state.db`.
- Redaction: user name, home directory, project path, host name and placeholder API keys replaced; path slugs renamed to match; SQLite text columns rewritten in place and the database checkpointed and vacuumed.
- Events 4, sessions 4, diagnostics 0, skipped 0.
- Totals: input 25491, output 377, cache read 57890, cache write 0, reasoning 0.
- Hermes needs a named provider with context_length of at least 64000; four turns, each with several tool-call requests, so the gateway logged 16 requests for 4 session rows.
- The `state_meta` rows `gateway.local_policy.v1:*` held the capture rig's environment (its PATH); they are blanked to `{}` because the reader does not read `state_meta`, and the prompt text keeps the project path `C:/Users/user/harnesses/target/p-limit` as conversation text.
