# Auranion CLI

CLI tool to configure Auranion integrations across Claude Desktop, Claude Code, Codex, and OpenCode.

## Installation

### Windows (PowerShell)

```powershell
irm https://raw.githubusercontent.com/keb-org/auranion-config/main/install.ps1 | iex
```

### macOS / Linux

```bash
curl -fsSL https://raw.githubusercontent.com/keb-org/auranion-config/main/install.sh | sh
```

## Usage

```bash
# Interactive setup
auranion config

# Reapply saved configuration
auranion config --apply

# Show status and diagnostics
auranion status

# Self-update to latest release
auranion update
```

## Refresh behavior

`auranion config --apply` refreshes every enabled integration without retoggling. Managed values are repaired even after edits; unrelated settings remain. Failures return nonzero, with remaining integrations still attempted.

`auranion update` runs config reapply through the installed binary, including same-version checks. When upgrading from an older updater, run `auranion config --apply` once afterward to ensure the new config code runs.

Claude Code/Desktop use four Claude slots; Codex CLI/Desktop use four GPT slots, each strongest to lightest. OpenCode/Hermes use four stable server-routed tiers (gigachad, chad, sigma, alpha). Codex desktop and CLI share native provider settings and preserve ChatGPT login. Restart desktop and start a new thread after applying.

## Administrator Reference

- [Combos](COMBOS.md) — Eight stable Claude/Codex combo IDs with server-side routing.
- [Model Slots](DESKTOP_MODEL_SLUGS.md) — Four slots per Claude/Codex integration, compatibility and verification limits.
- [Session Context](CONTEXT.md) — Canonical catalog and architectural records.

