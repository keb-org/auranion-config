# Auranion Config â€” session context

Last updated: 2026-08-13

This document records the durable state, decisions, and verified facts for the `auranion-config` Rust CLI that configures Auranion model routing across Claude Desktop, Claude Code, Codex, ChatGPT/Codex Desktop, and OpenCode.

## Project

- Rust CLI binary: `auranion` (Cargo package `auranion`).
- Commands: `auranion config` (interactive), `auranion config --apply` (noninteractive reapply), `auranion status`.
- Dependencies: `anyhow`, `clap`, `crossterm 0.29`, `dialoguer`, `directories`, `keyring`, `ratatui 0.30`, `serde`, `serde_json`, `toml_edit`.
- Interactive integration picker is Ratatui; saved-key confirm and password entry stay native (Dialoguer).

## Canonical eight models (order is user priority — grouped by family: GPT → Grok → Muse Spark → DeepSeek → Gemini → Qwen)

1. `cx/gpt-5.6-sol` â€” GPT 5.6 Sol â€” context 372k, output 128k, vision
2. `cx/gpt-5.6-terra` â€” GPT 5.6 Terra â€” context 272k, output 128k, vision
3. `cx/gpt-5.6-luna` â€” GPT 5.6 Luna â€” context 272k, output 128k, vision
4. `alibaba/qwen3.8-max` â€” Qwen 3.8 Max â€” context 1M, output 64k, no vision
5. `ag/gemini-3.6-flash-tiered` â€” Gemini 3.6 Flash â€” context 1M, output 64k, vision/audio/video
6. `cmc/deepseek/deepseek-v4-flash` â€” DeepSeek V4 Flash â€” context 1M, output 384k, no vision
7. `cmc/meta/muse-spark-1.2-contributor` â€” Muse Spark 1.2 â€” context 1M, output 128k, vision/audio/video

Retired (must never reappear in profiles): Poolside Laguna S 2.1, Poolside Laguna XS 2.1, GLM 5.2, DeepSeek V4 Pro, Qwen 3.7 Plus, Qwen 3.6 Flash.

## Reasoning-effort contracts (API-verified via 9router invalid-value probes; DeepSeek Pro from api-docs.deepseek.com/thinking_mode + Firecrawl)

- GPT-5.6 Sol / Terra / Luna: `none, minimal, low, medium, high, xhigh, max`
- Gemini 3.6 Flash: `low, medium, high`
- Qwen 3.8 Max: no effort control
- DeepSeek V4 Flash: `low, high, max` (thinking on by default at `high`; no off-toggle)
- DeepSeek V4 Pro: `low, high, max` (same thinking_mode toggle as Flash; 1M context, 393k max output on Novita)
- Muse Spark 1.2: `minimal, low, medium, high, xhigh` (`none` returns HTTP 400)
- Grok 4.6: `low, medium, high, xhigh` (default `high`, cannot disable; from docs.x.ai)

Model-level `max` = "Ultra" picker label. `ultra`/`ultracode` are app-level, never model options.

## Claude Desktop direct gateway (working, do not regress)

Config target: `%LOCALAPPDATA%\Claude-3p\configLibrary\<appliedId>.json` with `configLibrary\_meta.json` supplying `appliedId`.

Exact contract written by `merge_desktop`:
- `inferenceProvider`: `gateway`
- `inferenceCredentialKind`: `static`
- `inferenceGatewayBaseUrl`: `https://agent.auranion.com/v1`
- `inferenceGatewayApiKey`: saved Auranion key
- `inferenceGatewayAuthScheme`: `x-api-key`
- `modelDiscoveryEnabled`: `false`
- `inferenceModels`: nine entries `{ name: desktop_alias, labelOverride: desktop_label, supports1m }`

Removes obsolete `anthropicBaseUrl` / `anthropicApiKey`. No local proxy, supervisor, scheduled task, localhost listener, or certificate.

Claude Desktop picker routes (verified effort mapping):
- GPT 5.6 Sol â†’ `claude-opus-4-8`
- GPT 5.6 Terra â†’ `claude-opus-4-7`
- GPT 5.6 Luna â†’ `claude-opus-4-6`
- Qwen 3.8 Max â†’ `claude-opus-4-5-20251101`
- Gemini 3.6 Flash â†’ `claude-sonnet-4-6`
- DeepSeek V4 Flash â†’ `claude-haiku-4-5-20251001`
- Muse Spark 1.2 â†’ `claude-sonnet-4-5-20250920`

Effort-capable desktop aliases (render Effort control): opus 4.8/4.7/4.6/4.5-20251101 + sonnet-4-6. Active: Sol (opus-4-8), Grok 4.6 (opus-4-7, ultra→xhigh), Muse Spark 1.2 (opus-4-6, ultra→xhigh), DeepSeek V4 Pro (opus-4-5-20251101). Sonnet-4-6 spare. Non-effort routes carry `forced_effort`: Flash max, Terra max, Luna max, Gemini high, Qwen none.

Verified end-to-end: `claude-opus-4-8` returns upstream `gpt-5.6-sol`; `claude-sonnet-4-6` streams SSE HTTP 200.

## Claude Code

Writes `~/.claude/settings.json` env:
- `ANTHROPIC_BASE_URL`, `ANTHROPIC_API_KEY`, `ANTHROPIC_MODEL` (default `cx/gpt-5.6-sol`)
- `ANTHROPIC_DEFAULT_FABLE_MODEL` = `cx/gpt-5.6-luna`, `ANTHROPIC_DEFAULT_OPUS_MODEL` = `cx/gpt-5.6-sol`, `ANTHROPIC_DEFAULT_SONNET_MODEL` = `cx/gpt-5.6-terra`, `ANTHROPIC_DEFAULT_HAIKU_MODEL` = `cmc/deepseek/deepseek-v4-flash` (raw gateway IDs)
- Custom model option env describing the eight-model catalog.

## Codex / ChatGPT Desktop

Writes:
- `~/.codex/config.toml`: removes `preferred_auth_method`, global `model`, `model_provider`. Sets `model_catalog_json`, `[model_providers.auranion]` (base_url `https://agent.auranion.com/v1`, `env_key=OPENAI_API_KEY`, `wire_api=responses`), `agents.subagent` model `cx/gpt-5.6-sol`, named profiles for each model.
- `~/.codex/model-catalogs/auranion.json`: generated catalog. `supported_reasoning_levels` per model from the effort contract; `default_reasoning_level=medium` only when the model has efforts. No `default_reasoning_level` for Qwen/DeepSeek.
- `~/.codex/desktop-model-providers.json`: `version:1`, `default_provider:"openai"`, providers `openai` + `auranion`, and `model_providers` mapping all nine slugs to `auranion`.

Rules:
- Do NOT set global `model` or `model_provider`.
- Auranion profiles require Codex API-key mode. Selection replaces `auth.json` with `auth_mode=apikey` and the saved Auranion key; it must not run through signed-in ChatGPT mode.
- Deselect restores the complete pre-Auranion `auth.json`, including ChatGPT OAuth tokens.
- Better-Codex-App-Custom-Provider-Support repo (`D:\KEB\finance\old\Better-Codex-App-Custom-Provider-Support`) defines the provider-routing contract. Its app patch is macOS-only; no Windows Store app modification is authorized.

## OpenCode

Writes `~/.config/opencode/opencode.jsonc` (or `%USERPROFILE%\.config\opencode\opencode.jsonc` on Windows). Merges `provider.auranion` JSON with `name: Auranion`, `npm: @ai-sdk/openai-compatible`, `options.baseURL`, and per-model entries with `variants` from the effort contract. Qwen has no `variants`; DeepSeek variants are `low`/`high`/`max`.

`merge_opencode` is JSON-based and idempotent. It collapses duplicate `auranion` keys (previous string-merge corrupted the file with 22 stacked blocks; fixed). Auth via `~/.local/share/opencode/auth.json` `auranion` entry.

## Known current state

- Claude Desktop config working exceptionally; all models and effort controls correct. Locked by tests.
- Codex Desktop picker provider selection still requires the macOS-only Better-Codex app patch; stock Windows Store app ignores `desktop-model-providers.json`.
- API key was exposed in earlier console output. Rotate Auranion key after confirming.

## Decisions

- D-0031: direct 9router Claude Desktop gateway (supersedes D-0006, D-0030).
- D-0032: Codex Desktop provider-routing contract.
- D-0033: Ratatui integration picker.
- D-0034: per-model reasoning-effort contracts.

Decision records: `docs/stock-intelligence-engine/decisions/00NN-*.md` (project mirror) and the canonical copies under `D:\KEB\finance\docs\stock-intelligence-engine\decisions\`.

## Verification

- `cargo fmt`
- `cargo test` (16 tests pass)
- `cargo build --release`
- `.\target\release\auranion.exe config --apply`
- `.\target\release\auranion.exe status`
