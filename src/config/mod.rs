mod adapters;
mod integration;
mod io;
mod keyring;
mod state;
mod ui;

use anyhow::{Context, Result};
use directories::BaseDirs;
use std::{fs, io::Write};

use self::{integration::Integration, state::State};

pub(super) const BASE_URL: &str = "https://agent.auranion.com/v1";

pub(super) fn codex_desktop_routes() -> impl Iterator<Item = (&'static str, &'static str)> {
    crate::catalog::MODELS
        .iter()
        .map(|model| (model.codex_desktop_alias, model.upstream))
}

pub(super) fn configure() -> Result<()> {
    let dirs = BaseDirs::new().context("cannot determine user directories")?;
    let data_dir = dirs.data_local_dir().join("auranion");
    fs::create_dir_all(&data_dir)?;
    let mut state = load_state(&data_dir)?;
    let detected = Integration::ALL.map(|integration| adapters::detect(integration, &dirs));
    let defaults = Integration::ALL.map(|integration| state.active.contains(&integration));
    let wanted = ui::select_integrations(&detected, &defaults)?;
    let old = state.active.clone();
    let requires_key = wanted
        .iter()
        .any(|integration| needs_selection(*integration, &old));
    let previous_api_key = if old.iter().any(|integration| integration.is_codex())
        || wanted.iter().any(|integration| integration.is_codex())
    {
        keyring::load().ok().flatten()
    } else {
        None
    };
    let api_key = resolve_api_key(requires_key)?;
    for integration in old
        .iter()
        .copied()
        .filter(|integration| !integration.is_codex() && !wanted.contains(integration))
    {
        adapters::deselect(integration, &dirs, &data_dir, &mut state)?;
    }
    for integration in wanted
        .iter()
        .copied()
        .filter(|integration| !integration.is_codex() && needs_selection(*integration, &old))
    {
        adapters::select(integration, &dirs, &data_dir, &mut state, &api_key)?;
    }
    if old.iter().any(|integration| integration.is_codex())
        || wanted.iter().any(|integration| integration.is_codex())
    {
        adapters::reconcile_codex(
            &dirs,
            &data_dir,
            &mut state,
            &wanted,
            previous_api_key.as_deref(),
        )?;
    }

    state.active = wanted;
    state.save(&data_dir)?;
    state.complete_codex_transaction()?;
    state.save(&data_dir)?;
    if state.active.is_empty() {
        keyring::delete()?;
    }
    println!("Configuration complete. Run `auranion status` for diagnostics.");
    Ok(())
}

pub(super) fn apply_saved() -> Result<()> {
    let dirs = BaseDirs::new().context("cannot determine user directories")?;
    let data_dir = dirs.data_local_dir().join("auranion");
    fs::create_dir_all(&data_dir)?;
    let mut state = load_state(&data_dir)?;
    if state.active.is_empty() {
        anyhow::bail!("no saved integrations; run `auranion config` first");
    }
    let api_key = keyring::load()?.context("secure credential unavailable")?;
    let wanted = state.active.clone();
    for integration in wanted
        .iter()
        .copied()
        .filter(|integration| !integration.is_codex())
    {
        adapters::select(integration, &dirs, &data_dir, &mut state, &api_key)?;
    }
    if wanted.iter().any(|integration| integration.is_codex()) {
        adapters::reconcile_codex(&dirs, &data_dir, &mut state, &wanted, Some(&api_key))?;
    }
    state.save(&data_dir)?;
    state.complete_codex_transaction()?;
    state.save(&data_dir)?;
    println!("Saved integrations reapplied. Run `auranion status` for diagnostics.");
    Ok(())
}

fn load_state(data_dir: &std::path::Path) -> Result<State> {
    let mut state = State::load(data_dir)?;
    if state.recover_codex_transaction()? {
        state.save(data_dir)?;
        state.complete_codex_transaction()?;
        state.save(data_dir)?;
    }
    Ok(state)
}

pub(super) fn print_provider_token() -> Result<()> {
    let api_key = keyring::load()?.context("Auranion API key missing; run `auranion config`")?;
    write_provider_token(std::io::stdout().lock(), &api_key)
}

fn write_provider_token(mut output: impl Write, api_key: &str) -> Result<()> {
    output
        .write_all(api_key.as_bytes())
        .context("write Codex provider token")?;
    output
        .write_all(b"\n")
        .context("write Codex provider token")?;
    Ok(())
}

pub(super) fn status() -> Result<()> {
    let dirs = BaseDirs::new().context("cannot determine user directories")?;
    let data_dir = dirs.data_local_dir().join("auranion");
    let state = load_state(&data_dir)?;
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
            format!(" â€” {}", report.join(", "))
        };
        println!(
            "  {}: {}{}",
            integration.label(),
            if active { "configured" } else { "not selected" },
            details
        );
        if active && integration == Integration::CodexDesktop {
            println!("    App alias â†’ Auranion target");
            for (alias, target) in codex_desktop_routes() {
                println!("      {alias} â†’ {target}");
            }
        }
    }
    Ok(())
}

fn needs_selection(integration: Integration, old: &[Integration]) -> bool {
    !old.contains(&integration)
        || matches!(
            integration,
            Integration::ClaudeDesktop | Integration::CodexCli
        )
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
            [
                "Claude Desktop",
                "Claude Code",
                "ChatGPT / Codex Desktop",
                "Codex CLI",
                "OpenCode",
            ]
        );
    }

    #[test]
    fn provider_token_output_contains_only_the_token() {
        let mut output = Vec::new();
        write_provider_token(&mut output, "test-token").unwrap();
        assert_eq!(output, b"test-token\n");
    }

    #[test]
    fn desktop_routes_match_verified_aliases() {
        assert_eq!(
            codex_desktop_routes().collect::<Vec<_>>(),
            vec![
                ("gpt-5.6-sol", "cx/gpt-5.6-sol"),
                ("gpt-5.6-terra", "cx/gpt-5.6-terra"),
                ("gpt-5.6-luna", "cx/gpt-5.6-luna"),
                ("gpt-5.3", "gcli/grok-4.6"),
                ("gpt-5.3-mini", "cmc/meta/muse-spark-1.2-contributor"),
                ("gpt-5.4-mini", "cmc/deepseek/deepseek-v4-flash"),
                ("gpt-5.3-turbo", "cmc/deepseek/deepseek-v4-pro"),
                ("gpt-5.4", "ag/gemini-3.6-flash-tiered"),
                ("gpt-5.5", "alibaba/qwen3.8-max"),
            ]
        );
    }
}
