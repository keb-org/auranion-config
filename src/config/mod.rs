mod adapters;
mod integration;
mod io;
mod keyring;
mod state;
mod ui;

use anyhow::{Context, Result, bail};
use directories::BaseDirs;
use std::{fs, io::Write};

use self::{integration::Integration, state::State};

pub(super) const BASE_URL: &str = "https://agent.auranion.com/v1";

pub(super) fn codex_desktop_routes() -> impl Iterator<Item = (&'static str, &'static str)> {
    crate::catalog::CODEX_DESKTOP_MODELS
        .iter()
        .map(|&id| (id, id))
}

fn non_codex_targets(wanted: &[Integration]) -> Vec<Integration> {
    wanted
        .iter()
        .copied()
        .filter(|integration| !integration.is_codex())
        .collect()
}

fn requires_key_for_wanted(wanted: &[Integration]) -> bool {
    !wanted.is_empty()
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

pub(super) fn apply_saved() -> Result<()> {
    let dirs = BaseDirs::new().context("cannot determine user directories")?;
    let data_dir = dirs.data_local_dir().join("auranion");
    fs::create_dir_all(&data_dir)?;
    let mut state = load_state(&data_dir)?;
    if state.active.is_empty() {
        println!("No saved integrations; nothing to reapply.");
        return Ok(());
    }

    let wanted = state.active.clone();
    let api_key = keyring::load();
    reapply_integrations(&wanted, |integration| {
        let result = if integration.is_codex() {
            adapters::reconcile_codex(
                &dirs,
                &data_dir,
                &mut state,
                &wanted,
                api_key.as_ref().ok().and_then(|key| key.as_deref()),
            )
        } else {
            match &api_key {
                Ok(Some(key)) => adapters::select(integration, &dirs, &data_dir, &mut state, key),
                Ok(None) => Err(anyhow::anyhow!(
                    "secure credential missing; run `auranion config`"
                )),
                Err(error) => Err(anyhow::anyhow!("secure credential unavailable: {error:#}")),
            }
        };
        // Persist backups even after a partial adapter failure, before continuing.
        state.save(&data_dir)?;
        state.complete_codex_transaction()?;
        state.save(&data_dir)?;
        result
    })?;
    if wanted.iter().any(|integration| integration.is_codex()) {
        api_key?.context(
            "Codex config refreshed, but secure credential missing; run `auranion config`",
        )?;
    }
    println!("Saved integrations reapplied. Run `auranion status` for diagnostics.");
    Ok(())
}

fn reapply_integrations(
    wanted: &[Integration],
    mut apply: impl FnMut(Integration) -> Result<()>,
) -> Result<()> {
    let mut errors = Vec::new();
    // Codex shares files and a transaction; reconcile both selections once, last.
    for &integration in wanted
        .iter()
        .filter(|integration| !integration.is_codex())
        .chain(wanted.iter().find(|integration| integration.is_codex()))
    {
        if let Err(error) = apply(integration) {
            let label = if integration.is_codex() {
                "Codex"
            } else {
                integration.label()
            };
            errors.push(format!("{label}: {error:#}"));
        }
    }
    if !errors.is_empty() {
        bail!(
            "Failed to reapply saved integrations:\n{}",
            errors.join("\n")
        );
    }
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
            format!(" — {}", report.join(", "))
        };
        println!(
            "  {}: {}{}",
            integration.label(),
            if active { "configured" } else { "not selected" },
            details
        );
        if active && integration == Integration::CodexDesktop {
            println!("    App alias → Auranion target");
            for (alias, target) in codex_desktop_routes() {
                println!("      {alias} → {target}");
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
                "Hermes",
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
                ("gpt-6-astra", "gpt-6-astra"),
                ("gpt-5.6-sol", "gpt-5.6-sol"),
                ("gpt-5.6-terra", "gpt-5.6-terra"),
                ("gpt-5.6-luna", "gpt-5.6-luna"),
            ]
        );
    }

    #[test]
    fn reapply_attempts_every_enabled_integration_even_after_errors() {
        let mut state = State::default();
        state.active = Integration::ALL.to_vec();
        for _ in 0..2 {
            let mut attempted = Vec::new();
            let error = reapply_integrations(&state.active, |integration| {
                attempted.push(integration);
                if matches!(
                    integration,
                    Integration::ClaudeDesktop | Integration::OpenCode
                ) {
                    bail!("cannot patch config");
                }
                Ok(())
            })
            .unwrap_err();
            assert_eq!(
                attempted,
                vec![
                    Integration::ClaudeDesktop,
                    Integration::ClaudeCode,
                    Integration::OpenCode,
                    Integration::Hermes,
                    Integration::CodexDesktop,
                ]
            );
            let message = error.to_string();
            assert!(message.contains("Claude Desktop: cannot patch config"));
            assert!(message.contains("OpenCode: cannot patch config"));
        }
    }

    #[test]
    fn reapply_leaves_disabled_integrations_alone() {
        for wanted in [
            vec![],
            vec![Integration::CodexCli],
            vec![Integration::Hermes],
        ] {
            let mut attempted = Vec::new();
            reapply_integrations(&wanted, |integration| {
                attempted.push(integration);
                Ok(())
            })
            .unwrap();
            assert_eq!(attempted, wanted);
        }
    }

    #[test]
    fn every_enabled_integration_requires_gateway_credentials() {
        assert!(requires_key_for_wanted(&[Integration::ClaudeCode]));
        assert!(requires_key_for_wanted(&[Integration::OpenCode]));
        assert!(requires_key_for_wanted(&[
            Integration::ClaudeCode,
            Integration::CodexDesktop
        ]));
        assert!(requires_key_for_wanted(&[Integration::CodexDesktop]));
        assert!(requires_key_for_wanted(&[Integration::CodexCli]));
        assert!(requires_key_for_wanted(&[
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
