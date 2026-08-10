use anyhow::{Context, Result};
use directories::BaseDirs;
use serde_json::{Map, Value, json};
use std::path::{Path, PathBuf};
use toml_edit::{DocumentMut, Item};

use crate::catalog::{DEFAULT_MODEL, MODELS};

use super::super::{
    BASE_URL,
    io::{read_toml, write_json, write_toml},
    state::State,
};

const OWNED_ROOT_KEYS: [&str; 3] = ["model", "model_provider", "model_catalog_json"];
const OWNED_PROVIDER_KEYS: [&str; 4] = ["name", "base_url", "env_key", "wire_api"];
const OWNED_SUBAGENT_KEYS: [&str; 2] = ["description", "model"];

pub(super) fn detect(dirs: &BaseDirs) -> bool {
    config_path(dirs).exists() || auth_path(dirs).exists()
}

pub(super) fn diagnostics(dirs: &BaseDirs) -> Vec<String> {
    (!config_path(dirs).exists())
        .then_some("config missing".into())
        .into_iter()
        .collect()
}

pub(super) fn select(
    dirs: &BaseDirs,
    data_dir: &Path,
    state: &mut State,
    api_key: &str,
) -> Result<()> {
    let config = config_path(dirs);
    let catalog = catalog_path(dirs);
    let providers = desktop_providers_path(dirs);
    let auth = auth_path(dirs);
    state.backup(data_dir, &config)?;
    state.backup(data_dir, &catalog)?;
    state.backup(data_dir, &providers)?;
    state.backup(data_dir, &auth)?;
    merge_config(&config, &catalog, &providers)?;
    merge_auth(&auth, api_key)
}

pub(super) fn deselect(dirs: &BaseDirs, state: &mut State) -> Result<()> {
    let config = config_path(dirs);
    restore_toml_owned(&config, state)?;
    let catalog = catalog_path(dirs);
    state.restore_file(&catalog)?;
    let providers = desktop_providers_path(dirs);
    state.restore_file(&providers)?;
    let auth = auth_path(dirs);
    state.restore_file(&auth)?;
    Ok(())
}

fn config_path(dirs: &BaseDirs) -> PathBuf {
    dirs.home_dir().join(".codex").join("config.toml")
}

fn auth_path(dirs: &BaseDirs) -> PathBuf {
    dirs.home_dir().join(".codex").join("auth.json")
}

fn catalog_path(dirs: &BaseDirs) -> PathBuf {
    dirs.home_dir()
        .join(".codex")
        .join("model-catalogs")
        .join("auranion.json")
}

fn desktop_providers_path(dirs: &BaseDirs) -> PathBuf {
    dirs.home_dir()
        .join(".codex")
        .join("desktop-model-providers.json")
}

fn merge_config(path: &Path, catalog_path: &Path, providers_path: &Path) -> Result<()> {
    let mut document = read_toml(path)?;
    document.remove("preferred_auth_method");
    document.remove("model");
    document.remove("model_provider");
    document["model_catalog_json"] = catalog_path.to_string_lossy().into_owned().into();

    if !document.contains_key("model_providers") || !document["model_providers"].is_table() {
        document["model_providers"] = toml_edit::Item::Table(toml_edit::Table::new());
    }
    let provider = document["model_providers"]
        .as_table_mut()
        .context("model_providers is not a table")?;

    if !provider.contains_key("auranion") || !provider["auranion"].is_table() {
        provider["auranion"] = toml_edit::Item::Table(toml_edit::Table::new());
    }
    let auranion = provider["auranion"]
        .as_table_mut()
        .context("model_providers.auranion is not a table")?;
    auranion["name"] = "Auranion".into();
    auranion["base_url"] = BASE_URL.into();
    auranion["env_key"] = "OPENAI_API_KEY".into();
    auranion["wire_api"] = "responses".into();

    if !document.contains_key("agents") || !document["agents"].is_table() {
        document["agents"] = toml_edit::Item::Table(toml_edit::Table::new());
    }
    let agents = document["agents"]
        .as_table_mut()
        .context("agents is not a table")?;

    if !agents.contains_key("subagent") || !agents["subagent"].is_table() {
        agents["subagent"] = toml_edit::Item::Table(toml_edit::Table::new());
    }
    let subagent = agents["subagent"]
        .as_table_mut()
        .context("agents.subagent is not a table")?;
    subagent["description"] = "Auranion subagent".into();
    subagent["model"] = DEFAULT_MODEL.into();

    if !document.contains_key("profiles") || !document["profiles"].is_table() {
        document["profiles"] = toml_edit::Item::Table(toml_edit::Table::new());
    }
    let profiles = document["profiles"]
        .as_table_mut()
        .context("profiles is not a table")?;
    profiles.clear();
    for model in MODELS {
        let profile = profiles
            .entry(model.label)
            .or_insert_with(toml_edit::table)
            .as_table_mut()
            .context("model profile is not a table")?;
        profile["model"] = model.upstream.into();
        profile["model_provider"] = "auranion".into();
    }

    write_catalog(catalog_path)?;
    write_desktop_providers(providers_path)?;
    write_toml(path, &document)
}

fn write_desktop_providers(path: &Path) -> Result<()> {
    let model_providers = MODELS
        .iter()
        .map(|model| (model.upstream.into(), Value::String("auranion".into())))
        .collect::<Map<_, _>>();
    write_json(
        path,
        &json!({
            "version": 1,
            "default_provider": "openai",
            "providers": [
                {
                    "id": "openai",
                    "label": "ChatGPT / OpenAI",
                    "description": "Uses your signed-in ChatGPT account"
                },
                {
                    "id": "auranion",
                    "label": "Auranion",
                    "description": "Uses [model_providers.auranion] from config.toml"
                }
            ],
            "model_providers": model_providers
        }),
    )
}

fn write_catalog(path: &Path) -> Result<()> {
    let models = MODELS
        .iter()
        .enumerate()
        .map(|(priority, model)| {
            let mut entry = json!({
                "slug": model.upstream,
                "display_name": model.label,
                "description": format!("{} via Auranion", model.label),
                "supported_reasoning_levels": model
                    .reasoning_efforts
                    .iter()
                    .map(|effort| reasoning_preset(effort))
                    .collect::<Vec<_>>(),
                "shell_type": "shell_command",
                "visibility": "list",
                "supported_in_api": true,
                "priority": priority,
                "additional_speed_tiers": [],
                "service_tiers": [],
                "availability_nux": null,
                "upgrade": null,
                "base_instructions": "You are Codex, an agent working through the Auranion model gateway.",
                "include_skills_usage_instructions": false,
                "supports_reasoning_summary_parameter": true,
                "default_reasoning_summary": "none",
                "support_verbosity": true,
                "default_verbosity": "low",
                "apply_patch_tool_type": "freeform",
                "web_search_tool_type": "text_and_image",
                "truncation_policy": {"mode": "tokens", "limit": 10000},
                "supports_parallel_tool_calls": true,
                "supports_image_detail_original": model.vision,
                "context_window": model.context,
                "max_context_window": model.context,
                "effective_context_window_percent": 95,
                "experimental_supported_tools": [],
                "input_modalities": if model.vision { json!(["text", "image"]) } else { json!(["text"]) },
                "supports_search_tool": true,
                "use_responses_lite": true,
                "tool_mode": "code_mode_only",
                "multi_agent_version": "v2"
            });
            if !model.reasoning_efforts.is_empty() {
                entry["default_reasoning_level"] = "medium".into();
            }
            entry
        })
        .collect::<Vec<_>>();
    write_json(path, &json!({ "models": models }))
}

fn reasoning_preset(effort: &str) -> Value {
    let description = match effort {
        "none" => "No extra reasoning effort",
        "minimal" => "Minimal reasoning effort",
        "low" => "Fast responses with lighter reasoning",
        "medium" => "Balances speed and reasoning depth for everyday tasks",
        "high" => "Greater reasoning depth for complex problems",
        "xhigh" => "Extra high reasoning depth for complex problems",
        "max" => "Maximum reasoning depth for the hardest problems",
        _ => "Reasoning effort",
    };
    json!({ "effort": effort, "description": description })
}

fn merge_auth(path: &Path, api_key: &str) -> Result<()> {
    write_json(
        path,
        &json!({
            "auth_mode": "apikey",
            "OPENAI_API_KEY": api_key,
        }),
    )
}

fn restore_toml_owned(path: &Path, state: &State) -> Result<()> {
    let Some(baseline) = state.baseline_for(path) else {
        return Ok(());
    };

    let original = read_toml(&baseline)?;
    let mut current = read_toml(path)?;
    restore_root_keys(&mut current, &original);
    restore_table_keys(
        &mut current,
        &original,
        "model_providers",
        "auranion",
        &OWNED_PROVIDER_KEYS,
    )?;
    restore_table_keys(
        &mut current,
        &original,
        "agents",
        "subagent",
        &OWNED_SUBAGENT_KEYS,
    )?;
    restore_table(&mut current, &original, "profiles")?;
    write_toml(path, &current)
}

fn restore_root_keys(current: &mut DocumentMut, original: &DocumentMut) {
    for key in OWNED_ROOT_KEYS {
        if let Some(value) = original.get(key) {
            current[key] = value.clone();
        } else {
            current.remove(key);
        }
    }
}

fn restore_table_keys(
    current: &mut DocumentMut,
    original: &DocumentMut,
    parent: &str,
    child: &str,
    keys: &[&str],
) -> Result<()> {
    let original_child = original
        .get(parent)
        .and_then(Item::as_table)
        .and_then(|table| table.get(child))
        .and_then(Item::as_table);

    if !current.contains_key(parent) || !current[parent].is_table() {
        current[parent] = toml_edit::Item::Table(toml_edit::Table::new());
    }
    let current_parent = current[parent]
        .as_table_mut()
        .with_context(|| format!("{parent} is not a table"))?;

    if !current_parent.contains_key(child) || !current_parent[child].is_table() {
        current_parent[child] = toml_edit::Item::Table(toml_edit::Table::new());
    }
    let current_child = current_parent[child]
        .as_table_mut()
        .with_context(|| format!("{parent}.{child} is not a table"))?;

    for key in keys {
        if let Some(value) = original_child.and_then(|table| table.get(key)) {
            current_child[*key] = value.clone();
        } else {
            current_child.remove(key);
        }
    }
    Ok(())
}

fn restore_table(current: &mut DocumentMut, original: &DocumentMut, key: &str) -> Result<()> {
    if let Some(value) = original.get(key) {
        current[key] = value.clone();
    } else {
        current.remove(key);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::io::read_json;

    #[test]
    fn merge_handles_non_table_model_providers_gracefully() {
        let dir = std::env::temp_dir().join(format!("auranion-codex-nontable-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("config.toml");
        let catalog = dir.join("model-catalogs").join("auranion.json");
        let providers = dir.join("desktop-model-providers.json");
        std::fs::write(&path, "model_providers = \"invalid_string_value\"\n").unwrap();

        merge_config(&path, &catalog, &providers).unwrap();
        let document = read_toml(&path).unwrap();
        assert!(document.get("model_providers").unwrap().is_table());

        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn merge_auth_uses_api_key_mode_only() {
        let path =
            std::env::temp_dir().join(format!("auranion-codex-auth-{}.json", std::process::id()));
        merge_auth(&path, "auranion-key").unwrap();

        assert_eq!(
            read_json(&path).unwrap(),
            json!({
                "auth_mode": "apikey",
                "OPENAI_API_KEY": "auranion-key"
            })
        );

        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn merge_writes_canonical_profiles_and_catalog() {
        let dir = std::env::temp_dir().join(format!("auranion-codex-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("config.toml");
        let catalog = dir.join("model-catalogs").join("auranion.json");
        let providers = dir.join("desktop-model-providers.json");
        std::fs::write(&path, "preferred_auth_method = \"chatgpt\"\n").unwrap();

        merge_config(&path, &catalog, &providers).unwrap();
        let document = read_toml(&path).unwrap();
        let profiles = document.get("profiles").and_then(Item::as_table).unwrap();
        assert_eq!(profiles.len(), MODELS.len());
        for model in MODELS {
            let profile = profiles.get(model.label).and_then(Item::as_table).unwrap();
            assert_eq!(
                profile.get("model").and_then(Item::as_str),
                Some(model.upstream)
            );
            assert_eq!(
                profile.get("model_provider").and_then(Item::as_str),
                Some("auranion")
            );
        }
        assert!(document.get("preferred_auth_method").is_none());
        assert!(catalog.exists());
        assert!(providers.exists());

        let generated = read_json(&catalog).unwrap();
        let entries = generated.get("models").and_then(Value::as_array).unwrap();
        assert_eq!(entries.len(), MODELS.len());
        assert_eq!(
            entries[0].get("slug").and_then(Value::as_str),
            Some(DEFAULT_MODEL)
        );
        assert_eq!(
            entries[0]
                .get("supported_reasoning_levels")
                .and_then(Value::as_array)
                .map(Vec::len),
            Some(MODELS[0].reasoning_efforts.len())
        );

        std::fs::remove_dir_all(&dir).unwrap();
    }
}
