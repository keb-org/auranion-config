# Gateway combos

`agent.auranion.com` owns these eight stable combo IDs. Clients send the selected model ID unchanged. Upstream models, pools, fallback, and effort translation stay server-side.

Order: strongest to lightest within each family.

| Clients | Combo IDs in picker order |
| --- | --- |
| Claude Code / Claude Desktop | `claude-fable-5-1`, `claude-opus-5`, `claude-sonnet-5`, `claude-haiku-4-5-20251001` |
| Codex CLI / ChatGPT–Codex Desktop integration | `gpt-6-astra`, `gpt-5.6-sol`, `gpt-5.6-terra`, `gpt-5.6-luna` |

Changing upstream routes does not require client config changes. OpenCode and Hermes retain the separate eight-model `MODELS` catalog.

Native Codex Ultra is an orchestration mode: the tested backend sends `reasoning.effort: "max"` upstream. Auranion config does not rewrite model IDs or effort values.
