# Model slots

Four slots per Claude/Codex integration, strongest to lightest. Requests send IDs unchanged; gateway owns combo routing. Auranion writes `https://agent.auranion.com` to Claude Code's `ANTHROPIC_BASE_URL` and Desktop's `inferenceGatewayBaseUrl`. Codex, OpenCode, and Hermes retain `https://agent.auranion.com/v1`. Reapply replaces the old versioned Claude base without changing unrelated settings.

## Claude Code and Claude Desktop

| Model ID | Label |
| --- | --- |
| `claude-fable-5-1` | Claude Fable 5.1 |
| `claude-opus-5` | Claude Opus 5 |
| `claude-sonnet-5` | Claude Sonnet 5 |
| `claude-haiku-4-5-20251001` | Claude Haiku 4.5 |

Claude Desktop writes four `inferenceModels` in this order, with `modelDiscoveryEnabled: false`, `supports1m: false`, and existing direct gateway authentication.

Claude Code writes ordered `modelPicker.options`, `replaceBuiltInOptions: true`, the matching `availableModels` allowlist, and native Fable/Opus/Sonnet/Haiku role IDs. Ordered custom picker requires Claude Code **2.1.242+**. Native Default/current-session rows can remain; managed organization settings can override user settings.

## Codex CLI and ChatGPT / Codex Desktop

| Model ID | Native efforts |
| --- | --- |
| `gpt-6-astra` | low, medium, high, xhigh, max, ultra |
| `gpt-5.6-sol` | low, medium, high, xhigh, max, ultra |
| `gpt-5.6-terra` | low, medium, high, xhigh, max, ultra |
| `gpt-5.6-luna` | low, medium, high, xhigh, max |

Both use native `config.toml` with `model_provider = "auranion"`, default `model = "gpt-6-astra"`, and `model_catalog_json` pointing to `model-catalogs/auranion.json`. Catalog array order and priority agree. `model/list` supplies native model picker and effort choices; medium is catalog default. Ultra is native orchestration and sends max upstream.

Desktop and CLI share `CODEX_HOME` (default `~/.codex`), so selecting either configures their shared provider. A selected root `profile` is cleared while enabled to prevent it overriding the provider/catalog; profile definitions remain and deselection restores the original selector. Deprecated `preferred_auth_method` is removed while enabled. Provider command authentication obtains Auranion key from secure storage. Existing `auth.json` and ChatGPT OAuth stay unchanged, except restoration of provably old Auranion-generated auth.

Stock desktop does **not** consume `desktop-model-providers.json`. No new provider map is written. Previously recorded managed fields are restored/removed without deleting unrelated user fields. No app patch, `models_cache.json` edit, or local proxy.

Compatibility checked with Windows **OpenAI.Codex 26.915.3509.0**, backend **0.155.0-alpha.9**. Older backends whose reasoning enum rejects max/ultra require an app/CLI update. This integration configures Codex Desktop, not the separate consumer ChatGPT application.

## Reapply and verification

Reapply refreshes managed fields even after user edits, removes managed duplicates and retired `gpt-5.5`, and preserves unrelated settings and custom catalog entries. Four managed slots are not a promise to delete user-added models.

`tests/codex-native.mjs` uses an isolated temporary home, generated config, fake command-auth token, and loopback Responses fixture. It checks strict config parsing, four-model order, default provider, and all 23 model/effort combinations in one thread. No live gateway credentials or user app config are used.

Restart desktop and start a new conversation after applying. Existing threads can retain their original provider. Actual desktop UI clicks and live gateway inference remain separate end-to-end checks.
