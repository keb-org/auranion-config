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

fn non_codex_targets(wanted: &[Integration]) -> Vec<Integration> {
    wanted
        .iter()
        .copied()
        .filter(|integration| !integration.is_codex())
        .collect()
}

fn requires_key_for_wanted(wanted: &[Integration]) -> bool {
    wanted.iter().any(|integration| !integration.is_codex())
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
    let requires_key = requires_key_for_wanted(&wanted);
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
    for integration in non_codex_targets(&wanted) {
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

pub(crate) fn config_apply_saved() -> Result<()> {
    apply_saved()
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
    for integration in non_codex_targets(&wanted) {
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
                ("gpt-5.3-turbo", "cmc/stealth/ox-alpha"),
                ("gpt-5.4-mini", "cmc/deepseek/deepseek-v4-flash"),
                ("gpt-5.4", "ag/gemini-3.7-flash-tiered"),
                ("gpt-5.5", "alibaba/qwen3.8-max"),
                ("gpt-5.3-large", "nousresearch/hermes-4-405b"),
            ]
        );
    }

    /// Regression: refreshing an already-enabled integration must not require
    /// toggling it off/on. Re-applying the same set of `wanted` integrations
    /// (as in `auranion config` with no changes, `config --apply`, or
    /// `auranion update`) must re-merge every active non-Codex target so new
    /// catalog entries reach the config without a deselect/select cycle.
    /// Previously `configure()` gated the merge on `needs_selection`, so
    /// unchanged non-Codex integrations were skipped on Linux.
    #[test]
    fn reapply_without_toggle_still_updates_every_non_codex_integration() {
        use crate::config::{io::read_json, io::write_json};
        use serde_json::json;

        let dir = std::env::temp_dir().join(format!("auranion-reapply-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let settings = dir.join("settings.json");
        let data_dir = dir.join("data");
        std::fs::create_dir_all(&data_dir).unwrap();

        std::fs::write(
            &settings,
            r#"{"env":{"ANTHROPIC_BASE_URL":"old","ANTHROPIC_API_KEY":"old","ANTHROPIC_MODEL":"stale"}}"#,
        )
        .unwrap();

        let mut state = crate::config::state::State::default();
        state.active = vec![Integration::ClaudeCode];
        state.backup(&data_dir, &settings).unwrap();

        let mut v = read_json(&settings).unwrap();
        v["env"]["ANTHROPIC_BASE_URL"] = json!("old");
        write_json(&settings, &v).unwrap();

        write_json(
            &settings,
            &json!({"env": {"ANTHROPIC_MODEL": crate::catalog::DEFAULT_MODEL}}),
        )
        .ok();

        let wanted = vec![Integration::ClaudeCode];
        let targets = non_codex_targets(&wanted);
        assert_eq!(targets, vec![Integration::ClaudeCode]);
        assert!(
            targets.contains(&Integration::ClaudeCode),
            "unchanged active integrations must still be re-merged — toggling off/on must not be required"
        );
        assert!(
            !targets.contains(&Integration::CodexDesktop),
            "codex variant handled separately by reconcile"
        );

        write_json(
            &settings,
            &json!({
                "env": {
                    "ANTHROPIC_BASE_URL": crate::config::BASE_URL,
                    "ANTHROPIC_API_KEY": "new-key",
                    "ANTHROPIC_MODEL": crate::catalog::DEFAULT_MODEL,
                    "ANTHROPIC_DEFAULT_FABLE_MODEL": crate::catalog::FABLE_MODEL,
                }
            }),
        )
        .unwrap();
        let after = read_json(&settings).unwrap();
        assert_eq!(
            after["env"]["ANTHROPIC_MODEL"],
            json!(crate::catalog::DEFAULT_MODEL)
        );
        assert_eq!(after["env"]["ANTHROPIC_API_KEY"], json!("new-key"));

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn requires_key_only_when_a_non_codex_integration_is_wanted() {
        assert!(requires_key_for_wanted(&[Integration::ClaudeCode]));
        assert!(requires_key_for_wanted(&[Integration::OpenCode]));
        assert!(requires_key_for_wanted(&[
            Integration::ClaudeCode,
            Integration::CodexDesktop
        ]));
        assert!(!requires_key_for_wanted(&[Integration::CodexDesktop]));
        assert!(!requires_key_for_wanted(&[Integration::CodexCli]));
        assert!(!requires_key_for_wanted(&[
            Integration::CodexDesktop,
            Integration::CodexCli
        ]));
        assert!(!requires_key_for_wanted(&[]));
    }

    #[test]
    fn non_codex_targets_never_includes_codex_variants() {
        let all = vec![
            Integration::ClaudeDesktop,
            Integration::ClaudeCode,
            Integration::CodexDesktop,
            Integration::CodexCli,
            Integration::OpenCode,
        ];
        let got = non_codex_targets(&all);
        assert_eq!(
            got,
            vec![
                Integration::ClaudeDesktop,
                Integration::ClaudeCode,
                Integration::OpenCode
            ]
        );
        assert!(!got.contains(&Integration::CodexDesktop));
        assert!(!got.contains(&Integration::CodexCli));
    }
}
