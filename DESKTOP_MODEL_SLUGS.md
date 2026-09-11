# Desktop Model Slugs — Picker Acceptance

## Claude Desktop — accepted `desktop_alias` values

Claude Desktop routes by Anthropic alias. Any `claude-*` slug is accepted; Claude 5 slots (`claude-fable-5`, `claude-sonnet-5`) and Claude 4 effort-capable slots render reasoning/effort controls in the Model Picker.

| Slug (`desktop_alias`) | Route → upstream | Effort control |
| --- | --- | --- |
| `claude-opus-4-8` | `cx/gpt-6-astra` | Yes — user picks effort |
| `claude-opus-4-7` | `cx/gpt-5.6-terra` | Yes — user picks effort |
| `claude-sonnet-4-6` | `cx/gpt-5.6-luna` | Yes — user picks effort |
| `claude-opus-4-5-20251101` | `gcli/grok-4.6` | Yes (ultra→xhigh) |
| `claude-fable-5` | `cmc/meta/muse-spark-1.3-contributor` | Yes (adaptive / ultra→xhigh) |
| `claude-opus-4-6` | `cmc/z-ai/glm-5.3-flash` | Yes — user picks effort |
| `claude-haiku-4-5-20251001` | `deepseek/deepseek-v4.1-flash` | No — forced `max` |
| `claude-sonnet-5` | `ag/gemini-3.8-flash-tiered` | Yes (adaptive / low-high) |

Source: `src/catalog.rs` `MODELS[].desktop_alias`. Effort-capable set is `{ claude-fable-5, claude-sonnet-5, claude-opus-4-8, claude-opus-4-7, claude-opus-4-6, claude-opus-4-5-20251101, claude-sonnet-4-6 }`. DeepSeek routes on `claude-haiku-4-5-20251001` with `forced_effort: Some("max")`.

## ChatGPT / Codex Desktop — accepted `codex_desktop_alias` values

ChatGPT Desktop's Model Picker only renders **native `gpt-*` slugs** it knows about. This side uses identity routing (`codex_desktop_alias == upstream`); the router handles pools server-side, so the app shows the default native catalog with no customization.

### Official native slugs the picker will render (from `~/.codex/models_cache.json`)

| Native slug | Visibility | Reasoning |
| --- | --- | --- |
| `gpt-6-astra` | `list` | low / medium / high / xhigh / max / ultra |
| `gpt-5.6-sol` | `list` | low / medium / high / xhigh / max / ultra |
| `gpt-5.6-terra` | `list` | low / medium / high / xhigh / max / ultra |
| `gpt-5.6-luna` | `list` | low / medium / high / xhigh / max |
| `gpt-5.5` | `list` | *(none)* |

### Routing table actually written by `auranion config` (`codex_desktop_alias` → upstream)

The catalog `supported_reasoning_levels` is filtered through `CODEX_REASONING_EFFORTS` — the codex CLI enum only accepts `none / minimal / low / medium / high / xhigh` and fails to parse the whole catalog on `max`/`ultra`. The ChatGPT Desktop picker reads its own `models_cache.json` (native slugs above); it is not driven by these levels.

| App alias shown in picker | Routes to | Effort exposed to picker |
| --- | --- | --- |
| `gpt-6-astra` | `gpt-6-astra` | low / medium / high / xhigh / max / ultra |
| `gpt-5.6-sol` | `gpt-5.6-sol` | low / medium / high / xhigh / max / ultra |
| `gpt-5.6-terra` | `gpt-5.6-terra` | low / medium / high / xhigh / max / ultra |
| `gpt-5.6-luna` | `gpt-5.6-luna` | low / medium / high / xhigh / max |
| `gpt-5.5` | `gpt-5.5` | *(none)* |

See `COMBOS.md` for the matching 5 router combos.

### Verification

Before adding a new slug, confirm it is in `models_cache.json` or has been tested end-to-end: `auranion config --apply` → restart ChatGPT Desktop → select alias → send a message → confirm the gateway logs the expected `model=` value. Do not add aliases by string-harvesting binaries.
