mod claude_code;
mod claude_desktop;
mod codex;
mod opencode;

use anyhow::Result;
use directories::BaseDirs;
use std::path::Path;

use super::{integration::Integration, state::State};

pub(super) fn detect(integration: Integration, dirs: &BaseDirs) -> bool {
    match integration {
        Integration::ClaudeDesktop => claude_desktop::detect(dirs),
        Integration::ClaudeCode => claude_code::detect(dirs),
        Integration::Codex => codex::detect(dirs),
        Integration::OpenCode => opencode::detect(dirs),
    }
}

pub(super) fn diagnostics(integration: Integration, dirs: &BaseDirs) -> Vec<String> {
    match integration {
        Integration::ClaudeDesktop => claude_desktop::diagnostics(dirs),
        Integration::ClaudeCode => claude_code::diagnostics(dirs),
        Integration::Codex => codex::diagnostics(dirs),
        Integration::OpenCode => opencode::diagnostics(dirs),
    }
}

pub(super) fn select(
    integration: Integration,
    dirs: &BaseDirs,
    data_dir: &Path,
    state: &mut State,
    api_key: &str,
) -> Result<()> {
    match integration {
        Integration::ClaudeDesktop => claude_desktop::select(dirs, data_dir, state, api_key),
        Integration::ClaudeCode => claude_code::select(dirs, data_dir, state, api_key),
        Integration::Codex => codex::select(dirs, data_dir, state, api_key),
        Integration::OpenCode => opencode::select(dirs, data_dir, state, api_key),
    }
}

pub(super) fn deselect(integration: Integration, dirs: &BaseDirs, state: &mut State) -> Result<()> {
    match integration {
        Integration::ClaudeDesktop => claude_desktop::deselect(state),
        Integration::ClaudeCode => claude_code::deselect(dirs, state),
        Integration::Codex => codex::deselect(dirs, state),
        Integration::OpenCode => opencode::deselect(dirs, state),
    }
}
