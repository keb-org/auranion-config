use anyhow::Result;
use directories::BaseDirs;
use serde_json::{Value, json};
use std::path::{Path, PathBuf};

use crate::catalog::{DEFAULT_MODEL, FABLE_MODEL, HAIKU_MODEL, OPUS_MODEL, SONNET_MODEL};

use super::super::{
    BASE_URL,
    io::{json_object_mut, read_json, restore_json_fields, restore_object_keys, write_json},
    state::State,
};

const ENV_KEYS: [&str; 11] = [
    "ANTHROPIC_BASE_URL",
    "ANTHROPIC_API_KEY",
    "ANTHROPIC_MODEL",
    "ANTHROPIC_DEFAULT_FABLE_MODEL",
    "ANTHROPIC_DEFAULT_OPUS_MODEL",
    "ANTHROPIC_DEFAULT_SONNET_MODEL",
    "ANTHROPIC_DEFAULT_HAIKU_MODEL",
    "ANTHROPIC_CUSTOM_MODEL_OPTION",
    "ANTHROPIC_CUSTOM_MODEL_OPTION_NAME",
    "ANTHROPIC_CUSTOM_MODEL_OPTION_DESCRIPTION",
    "ANTHROPIC_CUSTOM_MODEL_OPTION_SUPPORTED_CAPABILITIES",
];

pub(super) fn detect(dirs: &BaseDirs) -> bool {
    settings_path(dirs).exists()
}

pub(super) fn diagnostics(dirs: &BaseDirs) -> Vec<String> {
    (!settings_path(dirs).exists())
        .then_some("settings missing".into())
        .into_iter()
        .collect()
}

pub(super) fn select(
    dirs: &BaseDirs,
    data_dir: &Path,
    state: &mut State,
    api_key: &str,
) -> Result<()> {
    let path = settings_path(dirs);
    state.backup(data_dir, &path)?;
    merge(&path, api_key)
}

pub(super) fn deselect(dirs: &BaseDirs, state: &mut State) -> Result<()> {
    let path = settings_path(dirs);
    restore_json_fields(&path, state, |current, original| {
        restore_object_keys(current, original, "env", &ENV_KEYS)
    })?;
    Ok(())
}

fn settings_path(dirs: &BaseDirs) -> PathBuf {
    dirs.home_dir().join(".claude").join("settings.json")
}

fn merge(path: &Path, api_key: &str) -> Result<()> {
    let mut settings = read_json(path)?;
    let environment = json_object_mut(&mut settings, "Claude Code settings root")?
        .entry("env")
        .or_insert_with(|| json!({}));
    let environment = json_object_mut(environment, "Claude Code settings env")?;
    let catalog = model_catalog_description();
    let values = [
        BASE_URL,
        api_key,
        DEFAULT_MODEL,
        FABLE_MODEL,
        OPUS_MODEL,
        SONNET_MODEL,
        HAIKU_MODEL,
        DEFAULT_MODEL,
        "Auranion model catalog",
        catalog.as_str(),
        "thinking",
    ];

    for (key, value) in ENV_KEYS.iter().zip(values) {
        environment.insert((*key).into(), Value::String(value.into()));
    }
    write_json(path, &settings)
}

fn model_catalog_description() -> String {
    crate::catalog::MODELS
        .iter()
        .map(|model| format!("{} ({})", model.label, model.upstream))
        .collect::<Vec<_>>()
        .join("; ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn merge_writes_raw_gateway_model_ids() {
        let path = std::env::temp_dir().join(format!(
            "auranion-claude-code-models-{}.json",
            std::process::id()
        ));
        std::fs::write(&path, "{}").unwrap();

        merge(&path, "key").unwrap();
        let settings = read_json(&path).unwrap();
        let environment = settings.get("env").and_then(Value::as_object).unwrap();
        assert_eq!(
            environment
                .get("ANTHROPIC_DEFAULT_FABLE_MODEL")
                .and_then(Value::as_str),
            Some(FABLE_MODEL)
        );
        assert_eq!(
            environment
                .get("ANTHROPIC_DEFAULT_OPUS_MODEL")
                .and_then(Value::as_str),
            Some(OPUS_MODEL)
        );
        assert_eq!(
            environment
                .get("ANTHROPIC_DEFAULT_SONNET_MODEL")
                .and_then(Value::as_str),
            Some(SONNET_MODEL)
        );
        assert_eq!(
            environment
                .get("ANTHROPIC_DEFAULT_HAIKU_MODEL")
                .and_then(Value::as_str),
            Some(HAIKU_MODEL)
        );
        assert_eq!(
            environment.get("ANTHROPIC_MODEL").and_then(Value::as_str),
            Some(DEFAULT_MODEL)
        );

        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn merge_rejects_non_object_root() {
        let path = std::env::temp_dir().join(format!(
            "auranion-claude-code-non-object-{}.json",
            std::process::id()
        ));
        std::fs::write(&path, "[]").unwrap();
        assert!(merge(&path, "key").is_err());
        std::fs::remove_file(path).unwrap();
    }
}
