# Auranion Config — current architecture

Updated: 2026-09-21. Package/binary: `auranion`, version 0.3.27.

## Catalogs

- Claude Code and Claude Desktop: Fable 5.1, Opus 5, Sonnet 5, Haiku 4.5, strongest to lightest.
- Codex CLI and Codex Desktop: GPT 6 Astra, GPT 5.6 Sol, Terra, Luna, strongest to lightest.
- Model IDs pass unchanged to the gateway. Claude Code/Desktop receive the origin-only base `https://agent.auranion.com`; Codex/OpenCode/Hermes retain `https://agent.auranion.com/v1`. Gateway owns upstream routing, pools, fallback, and effort translation. See [COMBOS.md](COMBOS.md).
- OpenCode and Hermes use four stable server-routed tiers, strongest to lightest: `auranion/gigachad`, `auranion/chad`, `auranion/sigma`, `auranion/alpha`. Upstream backends are configured gateway-side; client config never changes when backends swap.

## Claude

Claude Desktop retains direct third-party gateway config in `Claude-3p/configLibrary/<appliedId>.json`. Static `x-api-key` auth, discovery disabled, four ordered `inferenceModels`, `supports1m: false`. Owned profile metadata survives reapply. No localhost proxy or app modification.

Claude Code writes user settings: ordered `modelPicker` (requires 2.1.242+), four-model allowlist, initial Fable selection, and native role env IDs. Permissions and unrelated settings remain. Higher-priority managed settings can override user config.

## Codex native desktop and CLI

Both use shared `CODEX_HOME` and native `model_provider`, `model`, `model_catalog_json` in `config.toml`. Selecting either integration selects Auranion for the shared home. Provider uses Responses HTTP and command authentication via installed `auranion provider-token`; no API key is written to Codex config. `auth.json`/ChatGPT OAuth are not replaced. Legacy auth is restored only when ownership and previous key match.

Four catalog entries have native effort metadata, medium default, and increasing priorities. Tested backend supports max/ultra; native Ultra sends max upstream. Older enum-based backends may need upgrading.

`desktop-model-providers.json` belonged to a patched-app integration, not stock desktop. It is no longer generated. Recorded managed fields are restored/removed, preserving user additions. Deprecated `preferred_auth_method` and active root `profile` selector are removed while enabled; profile definitions remain. Restart desktop and use a new thread after switching providers.

This integration targets OpenAI Codex Desktop, not the separate consumer ChatGPT application. No Windows Store app modification is authorized or performed.

## Refresh and safety

`config --apply` patches managed values for every enabled integration, even if files changed or toggles did not. Other integrations still run after an adapter failure; command returns aggregated errors. Codex desktop/CLI reconcile once because they share files. Disabled integrations are not enabled implicitly, although their shared Codex home is inherently shared.

Original baselines remain immutable. Codex applies transactionally, preserves unrelated config/JSON fields, and records canonical JSON ownership separately from full expected transaction output. Edited JSON arrays are conservatively retained during deselection. Malformed config or filesystem failures are reported rather than overwritten silently.

`update` retains installed executable path, replaces binary, then launches that binary with `config --apply`. Same-version and failed update attempts also reapply current config. Failures return nonzero. First upgrade from an older updater can still execute old reapply code; run `auranion config --apply` explicitly once afterward.

## Verification scope

- Rust unit/regression tests cover catalog order, reapply, stale entry removal, auth preservation, rollback/recovery, and updater error propagation.
- `tests/codex-native.mjs` checks generated desktop-only config against installed backend: strict config parsing, provider selection, ordered `model/list`, command auth, and 23 model/effort requests in one thread to a local fixture.
- Installed version inspected: OpenAI.Codex 26.915.3509.0; backend 0.155.0-alpha.9.
- Native fixture uses isolated temp config and fake credentials, not user config or live gateway.
- Actual app UI interactions and live gateway inference are not verified. Installed client configs are not changed during verification.

## Documentation sources

- https://code.claude.com/docs/en/settings-reference#modelpicker
- https://code.claude.com/docs/en/model-config
- https://github.com/openai/codex/blob/main/codex-rs/protocol/src/openai_models.rs
- https://github.com/openai/codex/blob/main/codex-rs/model-provider-info/src/lib.rs

Installed native app/backend inspection takes precedence over historical patched-app assumptions.
