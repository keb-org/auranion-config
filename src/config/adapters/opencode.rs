use anyhow::Result;
use directories::BaseDirs;
use serde_json::{Map, Value, json};
use std::path::{Path, PathBuf};

use crate::catalog::{MODELS, Model};

use super::super::{
    BASE_URL,
    io::{json_object, json_object_mut, read_json, restore_json_fields, restore_key, write_json},
    state::State,
};

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
    let auth = auth_path(dirs);
    state.backup(data_dir, &config)?;
    state.backup(data_dir, &auth)?;
    merge_config(&config)?;
    merge_auth(&auth, api_key)
}

pub(super) fn deselect(dirs: &BaseDirs, state: &mut State) -> Result<()> {
    let config = config_path(dirs);
    restore_config(&config, state)?;
    let auth = auth_path(dirs);
    restore_json_fields(&auth, state, |current, original| {
        restore_key(current, original, "auranion")
    })?;
    Ok(())
}

fn config_path(dirs: &BaseDirs) -> PathBuf {
    let standard = dirs.config_dir().join("opencode").join("opencode.jsonc");
    let home = dirs
        .home_dir()
        .join(".config")
        .join("opencode")
        .join("opencode.jsonc");

    if standard.exists() {
        standard
    } else if home.exists() {
        home
    } else if cfg!(windows) {
        standard
    } else {
        home
    }
}

fn auth_path(dirs: &BaseDirs) -> PathBuf {
    let standard = dirs.data_local_dir().join("opencode").join("auth.json");
    let home = dirs
        .home_dir()
        .join(".local")
        .join("share")
        .join("opencode")
        .join("auth.json");

    if standard.exists() {
        standard
    } else if home.exists() {
        home
    } else if cfg!(windows) {
        standard
    } else {
        home
    }
}

fn merge_config(path: &Path) -> Result<()> {
    let mut root = read_json(path)?;
    let provider = json_object_mut(&mut root, "OpenCode config root")?
        .entry("provider")
        .or_insert_with(|| json!({}));
    let provider = json_object_mut(provider, "OpenCode provider")?;
    provider.insert("auranion".into(), auranion_provider());
    write_json(path, &root)
}

fn merge_auth(path: &Path, api_key: &str) -> Result<()> {
    let mut auth = read_json(path)?;
    json_object_mut(&mut auth, "OpenCode auth root")
        .map(|object| object.insert("auranion".into(), json!({ "type": "api", "key": api_key })))?;
    write_json(path, &auth)
}

fn restore_config(path: &Path, state: &State) -> Result<()> {
    let Some(baseline) = state.baseline_for(path) else {
        return Ok(());
    };

    let original = read_json(&baseline)?;
    let original_provider = json_object(&original, "OpenCode baseline root")?
        .get("provider")
        .and_then(Value::as_object)
        .and_then(|provider| provider.get("auranion"))
        .cloned();

    let mut current = read_json(path)?;
    let root = json_object_mut(&mut current, "OpenCode config root")?;
    let provider = root.entry("provider").or_insert_with(|| json!({}));
    let provider = json_object_mut(provider, "OpenCode provider")?;
    match original_provider {
        Some(provider_config) => {
            provider.insert("auranion".into(), provider_config);
        }
        None => {
            provider.remove("auranion");
        }
    }
    write_json(path, &current)
}

fn auranion_provider() -> Value {
    let models = MODELS
        .iter()
        .map(|model| (model.upstream.to_string(), model_config(model)))
        .collect::<Map<_, _>>();
    json!({
        "name": "Auranion",
        "npm": "@ai-sdk/openai-compatible",
        "options": { "baseURL": BASE_URL },
        "models": models,
    })
}

fn model_config(model: &Model) -> Value {
    let mut value = Map::new();
    value.insert("name".into(), Value::String(model.label.into()));

    if model.reasoning {
        value.insert("reasoning".into(), Value::Bool(true));
        let variants = model
            .reasoning_efforts
            .iter()
            .map(|effort| ((*effort).into(), json!({ "reasoningEffort": effort })))
            .collect::<Map<_, _>>();
        if !variants.is_empty() {
            value.insert("variants".into(), Value::Object(variants));
        }
    }

    if model.vision || model.audio || model.video {
        let mut input = vec![Value::String("text".into())];
        if model.vision {
            input.push(Value::String("image".into()));
        }
        if model.audio {
            input.push(Value::String("audio".into()));
        }
        if model.video {
            input.push(Value::String("video".into()));
        }
        value.insert("attachment".into(), Value::Bool(true));
        value.insert(
            "modalities".into(),
            json!({ "input": input, "output": ["text"] }),
        );
    }

    if model.context.is_some() || model.output.is_some() {
        let mut limit = Map::new();
        if let Some(context) = model.context {
            limit.insert("context".into(), context.into());
        }
        if let Some(output) = model.output {
            limit.insert("output".into(), output.into());
        }
        value.insert("limit".into(), Value::Object(limit));
    }
    Value::Object(value)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn merge_is_idempotent_and_preserves_other_providers() {
        let dir = std::env::temp_dir().join(format!("auranion-opencode-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("opencode.jsonc");
        std::fs::write(
            &path,
            r#"{
                "provider": {
                    "other": { "name": "Other" },
                    "auranion": { "old": true },
                    "auranion": { "older": true }
                }
            }"#,
        )
        .unwrap();

        merge_config(&path).unwrap();
        let first = std::fs::read(&path).unwrap();
        merge_config(&path).unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), first);

        let root = read_json(&path).unwrap();
        let provider = root.get("provider").and_then(Value::as_object).unwrap();
        assert!(provider.contains_key("other"));
        let auranion = provider.get("auranion").and_then(Value::as_object).unwrap();
        assert_eq!(
            auranion.get("name").and_then(Value::as_str),
            Some("Auranion")
        );
        assert_eq!(
            auranion
                .get("models")
                .and_then(Value::as_object)
                .map(Map::len),
            Some(MODELS.len())
        );

        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn merge_rejects_non_object_root() {
        let path = std::env::temp_dir().join(format!(
            "auranion-opencode-non-object-{}.json",
            std::process::id()
        ));
        std::fs::write(&path, "[]").unwrap();
        assert!(merge_config(&path).is_err());
        std::fs::remove_file(path).unwrap();
    }
}
