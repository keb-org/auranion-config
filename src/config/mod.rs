mod adapters;
mod integration;
mod io;
mod keyring;
mod state;
mod ui;

use anyhow::{Context, Result};
use directories::BaseDirs;
use std::fs;

use self::{integration::Integration, state::State};

pub(super) const BASE_URL: &str = "https://agent.auranion.com/v1";

pub(super) fn configure() -> Result<()> {
    let dirs = BaseDirs::new().context("cannot determine user directories")?;
    let data_dir = dirs.data_local_dir().join("auranion");
    fs::create_dir_all(&data_dir)?;
    let mut state = State::load(&data_dir)?;
    let detected = Integration::ALL.map(|integration| adapters::detect(integration, &dirs));
    let defaults = Integration::ALL.map(|integration| state.active.contains(&integration));
    let wanted = ui::select_integrations(&detected, &defaults)?;
    let old = state.active.clone();
    let requires_key = wanted
        .iter()
        .any(|integration| needs_selection(*integration, &old));
    let api_key = resolve_api_key(requires_key)?;
    for integration in old
        .iter()
        .copied()
        .filter(|integration| !wanted.contains(integration))
    {
        adapters::deselect(integration, &dirs, &mut state)?;
    }
    for integration in wanted
        .iter()
        .copied()
        .filter(|integration| needs_selection(*integration, &old))
    {
        adapters::select(integration, &dirs, &data_dir, &mut state, &api_key)?;
    }

    state.active = wanted;
    if state.active.is_empty() {
        keyring::delete()?;
    }
    state.save(&data_dir)?;
    println!("Configuration complete. Run `auranion status` for diagnostics.");
    Ok(())
}

pub(super) fn apply_saved() -> Result<()> {
    let dirs = BaseDirs::new().context("cannot determine user directories")?;
    let data_dir = dirs.data_local_dir().join("auranion");
    fs::create_dir_all(&data_dir)?;
    let mut state = State::load(&data_dir)?;
    if state.active.is_empty() {
        anyhow::bail!("no saved integrations; run `auranion config` first");
    }
    let api_key = keyring::load()?.context("secure credential unavailable")?;
    for integration in state.active.clone() {
        adapters::select(integration, &dirs, &data_dir, &mut state, &api_key)?;
    }
    state.save(&data_dir)?;
    println!("Saved integrations reapplied. Run `auranion status` for diagnostics.");
    Ok(())
}

pub(super) fn status() -> Result<()> {
    let dirs = BaseDirs::new().context("cannot determine user directories")?;
    let data_dir = dirs.data_local_dir().join("auranion");
    let state = State::load(&data_dir)?;
    let key_state = match keyring::load() {
        Ok(Some(_)) => "available",
        Ok(None) => "missing",
        Err(_) => "unavailable",
    };

    println!("Auranion status");
    println!("  Endpoint: {BASE_URL}");
    println!("  Credential: {key_state}");
    for integration in Integration::ALL {
        let active = state.active.contains(&integration);
        let report = adapters::diagnostics(integration, &dirs);
        let details = if report.is_empty() {
            String::new()
        } else {
            format!(" — {}", report.join(", "))
        };
        println!(
            "  {}: {}{}",
            integration.label(),
            if active { "configured" } else { "not selected" },
            details
        );
    }
    Ok(())
}

fn needs_selection(integration: Integration, old: &[Integration]) -> bool {
    !old.contains(&integration)
        || matches!(integration, Integration::ClaudeDesktop | Integration::Codex)
}

fn resolve_api_key(requires_key: bool) -> Result<String> {
    if !requires_key {
        return Ok(String::new());
    }

    let saved_key = keyring::load()?;
    match saved_key {
        Some(saved) => {
            let masked = mask_key(&saved);
            if ui::confirm_use_saved_key(&masked)? {
                Ok(saved)
            } else {
                let key = ui::prompt_api_key_tui()?;
                keyring::save(&key)?;
                Ok(key)
            }
        }
        None => {
            let key = ui::prompt_api_key_tui()?;
            keyring::save(&key)?;
            Ok(key)
        }
    }
}

fn mask_key(key: &str) -> String {
    if key.len() <= 8 {
        "*".repeat(key.len())
    } else {
        format!("{}...{}", &key[..4], &key[key.len() - 4..])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn integration_labels_are_stable() {
        assert_eq!(
            Integration::ALL.map(Integration::label),
            ["Claude Desktop", "Claude Code", "Codex", "OpenCode"]
        );
    }
}
