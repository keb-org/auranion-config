mod claude_code;
mod claude_desktop;
mod codex;
mod hermes;
mod opencode;

use anyhow::Result;
use directories::BaseDirs;
use std::path::Path;

use super::{integration::Integration, state::State};

pub(super) fn detect(integration: Integration, dirs: &BaseDirs) -> bool {
    match integration {
        Integration::ClaudeDesktop => claude_desktop::detect(dirs),
        Integration::ClaudeCode => claude_code::detect(dirs),
        Integration::CodexDesktop | Integration::CodexCli => codex::detect(dirs),
        Integration::OpenCode => opencode::detect(dirs),
        Integration::Hermes => hermes::detect(dirs),
    }
}

pub(super) fn diagnostics(integration: Integration, dirs: &BaseDirs) -> Vec<String> {
    match integration {
        Integration::ClaudeDesktop => claude_desktop::diagnostics(dirs),
        Integration::ClaudeCode => claude_code::diagnostics(dirs),
        Integration::CodexDesktop => codex::desktop_diagnostics(dirs),
        Integration::CodexCli => codex::diagnostics(dirs),
        Integration::OpenCode => opencode::diagnostics(dirs),
        Integration::Hermes => hermes::diagnostics(dirs),
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
        Integration::CodexDesktop | Integration::CodexCli => {
            unreachable!("Codex integrations use shared reconciliation")
        }
        Integration::OpenCode => opencode::select(dirs, data_dir, state, api_key),
        Integration::Hermes => hermes::select(dirs, data_dir, state, api_key),
    }
}

pub(super) fn reconcile_codex(
    dirs: &BaseDirs,
    data_dir: &Path,
    state: &mut State,
    wanted: &[Integration],
    previous_api_key: Option<&str>,
) -> Result<()> {
    codex::reconcile(dirs, data_dir, state, wanted, previous_api_key)
}

pub(super) fn deselect(
    integration: Integration,
    dirs: &BaseDirs,
    _data_dir: &Path,
    state: &mut State,
) -> Result<()> {
    match integration {
        Integration::ClaudeDesktop => claude_desktop::deselect(state),
        Integration::ClaudeCode => claude_code::deselect(dirs, state),
        Integration::CodexDesktop | Integration::CodexCli => {
            unreachable!("Codex integrations use shared reconciliation")
        }
        Integration::OpenCode => opencode::deselect(dirs, state),
        Integration::Hermes => hermes::deselect(dirs, state),
    }
}
