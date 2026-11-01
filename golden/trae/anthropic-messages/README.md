# trae / anthropic-messages

Real capture of a multi-turn session, promoted from the harness home directory with the verification skill's promotion script.

- Wire shape: anthropic-messages.
- Gateway: verify-skopli scripts/gateway.ts (usage rewrite, Responses pass-through).
- Server: llama-server b11720 (macOS arm64, CPU only), 2 slots, prompt cache on.
- Model: qwen2.5-1.5b-instruct-q4_k_m (served as "qwen").
- Cache numbers: server-reported.
- Store paths: `trajectories`.
- Redaction: user name, home directory, project path, host name and placeholder API keys replaced; path slugs renamed to match; SQLite text columns rewritten in place and the database checkpointed and vacuumed.
- Events 18, sessions 4, diagnostics 0, skipped 0.
- Totals: input 1468, output 3939, cache read 37343, cache write 0, reasoning 0.
- Captured on macOS 26.6.2 (arm64) with trae-agent 0.1.0 from the main branch (uv tool install, Python 3.12); the layout is the trajectory JSON the Ubuntu fixture hand-shaped, so this is the harness's only real case.
- Four runs of trae-cli run with --trajectory-file under the home, one trajectory per run, on the Anthropic wire shape because the Chat Completions client crashes on a null completion_tokens_details.
- Each run is a one-shot answer that trae nagged to its step cap (max_steps 6, every run ends success false); the 18 interactions hold one tool call in total and no file reads, so this is not a tool-loop fixture.
- Reader gap on record: trae writes naive local timestamps (datetime.now().isoformat(), no offset) and the reader books them as UTC, so these events sit 8 hours early for a capture made at UTC-8; the Ubuntu fixture pins the same behaviour with hand-shaped data.
- The gateway logged 19 requests and the trajectories hold 18 llm_interactions: the last call of run 4 hit max_tokens and trae recorded no interaction for it.
