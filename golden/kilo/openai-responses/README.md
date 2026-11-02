# kilo / openai-responses

Real capture of a multi-turn session, promoted from the harness home directory with the verification skill's promotion script.

- Wire shape: openai-responses.
- Gateway: usage-rewriting gateway in front of a local llama-server.
- Server: llama-server b11720 (Windows x64, CPU only), 2 slots, prompt cache on.
- Model: local model.
- Cache numbers: server-reported.
- Store paths: `.local/share/kilo/kilo.db`, `.local/share/kilo/storage`, `.local/share/kilo/log`.
- Redaction: user name, home directory, project path, host name and placeholder API keys replaced; path slugs renamed to match; SQLite text columns rewritten in place and the database checkpointed and vacuumed.
- Events 4, sessions 4, diagnostics 0, skipped 0.
- Totals: input 0, output 0, cache read 0, cache write 0, reasoning 0.
- Every assistant message in `kilo.db` carries kilo's `UnknownError` (`text part ... not found`) and zero tokens: kilo 7.8.18 stores no usage on the Responses wire, so the four events pin the reader's handling of errored, usage-less rows, and a reconcile against the gateway log differs by the full amount.
- The case stays as the Responses-wire contract until kilo records Responses usage and the case is recaptured.
