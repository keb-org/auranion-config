# Gateway combos

`agent.auranion.com` owns these eight stable combo IDs. Clients send the selected model ID unchanged. Upstream models, pools, fallback, and effort translation stay server-side.

Order: strongest to lightest within each family.

| Clients | Combo IDs in picker order |
| --- | --- |
| Claude Code / Claude Desktop | `claude-fable-5-1`, `claude-opus-5`, `claude-sonnet-5`, `claude-haiku-4-5-20251001` |
| Codex CLI / ChatGPT–Codex Desktop integration | `gpt-6-astra`, `gpt-5.6-sol`, `gpt-5.6-terra`, `gpt-5.6-luna` |

Changing upstream routes does not require client config changes. OpenCode and Hermes use the separate four-tier `MODELS` catalog (`gigachad`, `chad`, `sigma`, `alpha`, strongest to lightest), routed server-side. These are bare gateway model IDs: sending `auranion/chad` asks for provider credentials instead of the `chad` combo. OpenCode's local selector is still `auranion/chad` (provider/model); Hermes uses provider `auranion` with default model `chad`. Reapply migrates old namespaced tier selections in managed config; restart clients and reselect a tier in existing sessions.

Native Codex Ultra is an orchestration mode: the tested backend sends `reasoning.effort: "max"` upstream. Auranion config does not rewrite model IDs or effort values.
