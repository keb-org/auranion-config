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
