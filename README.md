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

## Automatic updates

After successful interactive `auranion config`, a per-user OS task checks for updates daily at 00:00 (midnight) local time. No daemon, administrator access, or regular CLI use required. `config --apply` does not register tasks.

| Platform | Scheduler | Architectures |
| --- | --- | --- |
| Windows | Task Scheduler; daily plus logon, missed-run catch-up | ARM64, AMD64 |
| macOS | LaunchAgent; daily plus login/registration | Apple Silicon, Intel |
| Linux | User systemd timer; persistent, up to 15 minutes jitter | ARM64, AMD64 |

Linux binaries target GNU/glibc (Ubuntu 22.04 build baseline), not Alpine/musl. Linux falls back to user crontab when no user systemd manager is available. Cron must be running; it cannot catch up missed runs. Windows/macOS tasks require a logged-in user; Linux user timers require a running user manager. Sleeping/offline machines cannot update until available again.

Disable automatic updates, including after later setup runs:

```bash
auranion schedule disable
```

Enable again, or activate on an existing installation after upgrading to a binary with this feature:

```bash
auranion schedule enable
```

Run setup/enable from the same environment as your integrations. Tasks preserve config path settings (`CODEX_HOME`, `HERMES_HOME`, Unix home/XDG roots, Windows `LOCALAPPDATA`), not credentials or the full shell environment. Rerun setup/enable after changing those paths or moving the binary; use the same home/XDG roots when disabling.

The binary must be installed in a user-writable directory. Updates verify release size, GitHub SHA-256 digest, and executable version before replacement, then reapply saved integrations using the installed binary. These checks do not protect against a compromised release publisher. Failed attempts retry at the next scheduled run; `auranion update` exposes errors interactively. Scheduler registration failure warns without undoing successful integration setup.

## Refresh behavior

`auranion config --apply` refreshes every enabled integration without retoggling. Managed values are repaired even after edits; unrelated settings remain. Failures return nonzero, with remaining integrations still attempted.

`auranion update` runs config reapply through the installed binary, including same-version checks. When upgrading from an older updater, run `auranion config --apply` once afterward to ensure the new config code runs.

Claude Code/Desktop use four Claude slots; Codex CLI/Desktop use four GPT slots, each strongest to lightest. OpenCode/Hermes use four stable server-routed tiers (gigachad, chad, sigma, alpha). Codex desktop and CLI share native provider settings and preserve ChatGPT login. Restart desktop and start a new thread after applying.

## Administrator Reference

- [Combos](COMBOS.md) — Eight stable Claude/Codex combo IDs with server-side routing.
- [Model Slots](DESKTOP_MODEL_SLUGS.md) — Four slots per Claude/Codex integration, compatibility and verification limits.
- [Session Context](CONTEXT.md) — Canonical catalog and architectural records.

