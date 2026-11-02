# commandcode / openai-chat

Real capture of a multi-turn session, promoted from the harness home directory with the verification skill's promotion script.

- Wire shape: openai-chat.
- Gateway: verify-skopli scripts/gateway.ts (usage rewrite, Responses pass-through).
- Server: llama-server b11720 (Linux x64, CPU only), 4 slots, prompt cache on.
- Model: qwen2.5-1.5b-instruct-q4_k_m (served as "qwen" through the openai-completions provider kind).
- Cache numbers: server-reported prompt_tokens_details.cached_tokens from the gateway (timings.cache_n).
- Store paths: `.commandcode/projects`.
- Redaction: user name, home directory, project path, host name and placeholder API keys replaced; path slugs renamed to match.
- Events 4, sessions 1, diagnostics 0, skipped 0.
- Totals: input 7738, output 262, cache read 51242, cache write 0, reasoning 0.
- Command Code 1.84.0 (--local-only --skip-onboarding --yolo -p, one session continued with -c); one <session-uuid>.jsonl per session under projects/<cwd-slug>/, the .checkpoints.jsonl and .meta.json beside it carry no usage.
- usage.inputTokens is the full prompt including the cached prefix (it equals the gateway's prompt_tokens) and usage.cacheReadTokens the cached part, so the reader books inputTokens minus cacheReadTokens as input; the totals match the gateway log exactly over the four turns (six requests, two of them aborted by the harness's retry and never recorded).
