# trae / anthropic-messages

Real capture of a multi-turn session, promoted from the harness home directory with the verification skill's promotion script.

- Wire shape: anthropic-messages.
- Gateway: verify-skopli scripts/gateway.ts (usage rewrite, Responses pass-through).
- Server: llama-server b11720 (macOS arm64, CPU only), 2 slots, prompt cache on.
- Model: qwen2.5-1.5b-instruct-q4_k_m (served as "qwen").
- Cache numbers: server-reported.
- Store paths: `trajectories`.
- Redaction: user name, home directory, project path, host name and placeholder API keys replaced; path slugs renamed to match.
- Events 18, sessions 4, diagnostics 4, skipped 0.
- Totals: input 1468, output 3939, cache read 37343, cache write 0, reasoning 0.
- Captured on macOS 26.6.2 (arm64) with trae-agent 0.1.0 from the main branch (uv tool install, Python 3.12); the layout is the trajectory JSON the Ubuntu fixture hand-shaped, so this is the harness's only real case.
- Four runs of trae-cli run with --trajectory-file under the home, one trajectory per run, on the Anthropic wire shape because the Chat Completions client crashes on a null completion_tokens_details.
- Each run is a one-shot answer that trae nagged to its step cap (max_steps 6, every run ends success false); the 18 interactions hold one tool call in total and no file reads, so this is not a tool-loop fixture.
- Timestamps: trae writes naive local wall-clock time (datetime.now().isoformat(), no offset); this capture was made at UTC-8, so the stamps read eight hours early as UTC.
- On a live machine the reader recovers the offset from each trajectory's file mtime, which trae sets when it writes the finished run: mtime minus `end_time` (or the newest timestamp when `end_time` is absent), rounded to the quarter-hour, when the gap is at most 14 hours and within ten minutes of the grid.
- A checkout gives these files the mtime of the checkout, which is not evidence; the replay runners pin every input mtime to 2000-01-01, so the golden replay has no usable anchor and intentionally books the naive timestamps as UTC with one `naive timestamps, no usable anchor` warning per trajectory (the four diagnostics below).
- The hand-shaped trae/basic fixture pins the same fallback.
- The gateway logged 19 requests and the trajectories hold 18 llm_interactions: the last call of run 4 hit max_tokens and trae recorded no interaction for it.
