use anyhow::{Context, Result};
use directories::BaseDirs;
use serde_json::{Value, json};
use std::path::{Path, PathBuf};

use crate::catalog::MODELS;

use super::super::{
    BASE_URL,
    io::{json_object_mut, read_json, restore_json_fields, restore_key, write_json},
    state::State,
};

const OWNED_KEYS: [&str; 9] = [
    "inferenceProvider",
    "inferenceCredentialKind",
    "inferenceGatewayBaseUrl",
    "inferenceGatewayApiKey",
    "inferenceGatewayAuthScheme",
    "modelDiscoveryEnabled",
    "inferenceModels",
    "anthropicBaseUrl",
    "anthropicApiKey",
];

pub(super) fn detect(dirs: &BaseDirs) -> bool {
    active_config_path(dirs).is_ok()
}

pub(super) fn diagnostics(_dirs: &BaseDirs) -> Vec<String> {
    Vec::new()
}

pub(super) fn select(
    dirs: &BaseDirs,
    data_dir: &Path,
    state: &mut State,
    api_key: &str,
) -> Result<()> {
    let path = active_config_path(dirs)?;
    state.backup(data_dir, &path)?;
    state.backup(data_dir, &metadata_path(dirs))?;
    merge(&path, api_key)?;
    state.desktop_config = Some(path);
    state.startup_artifact = None;
    Ok(())
}

pub(super) fn deselect(state: &mut State) -> Result<()> {
    let Some(path) = state.desktop_config.clone() else {
        return Ok(());
    };

    restore_json_fields(&path, state, |current, original| {
        for key in OWNED_KEYS {
            restore_key(current, original, key)?;
        }
        Ok(())
    })?;
    state.desktop_config = None;
    state.startup_artifact = None;
    Ok(())
}

fn metadata_path(dirs: &BaseDirs) -> PathBuf {
    config_root(dirs).join("configLibrary").join("_meta.json")
}

fn active_config_path(dirs: &BaseDirs) -> Result<PathBuf> {
    let meta_path = metadata_path(dirs);
    let metadata = read_json(&meta_path)?;
    let applied_id = metadata
        .get("appliedId")
        .and_then(Value::as_str)
        .context("Claude Desktop config metadata has no appliedId")?;
    if applied_id.is_empty() || applied_id.contains(['/', '\\', ':']) {
        anyhow::bail!("Claude Desktop config metadata has an invalid appliedId");
    }

    Ok(config_root(dirs)
        .join("configLibrary")
        .join(format!("{applied_id}.json")))
}

fn config_root(dirs: &BaseDirs) -> PathBuf {
    if cfg!(windows) {
        std::env::var_os("LOCALAPPDATA")
            .map(PathBuf::from)
            .unwrap_or_else(|| dirs.data_local_dir().to_path_buf())
            .join("Claude-3p")
    } else if cfg!(target_os = "macos") {
        dirs.home_dir()
            .join("Library")
            .join("Application Support")
            .join("Claude-3p")
    } else {
        dirs.home_dir().join(".config").join("Claude-3p")
    }
}

fn merge(path: &Path, api_key: &str) -> Result<()> {
    let mut config = read_json(path)?;
    let object = json_object_mut(&mut config, "Claude Desktop config root")?;
    object.insert("inferenceProvider".into(), "gateway".into());
    object.insert("inferenceCredentialKind".into(), "static".into());
    object.insert("inferenceGatewayBaseUrl".into(), BASE_URL.into());
    object.insert("inferenceGatewayApiKey".into(), api_key.into());
    object.insert("inferenceGatewayAuthScheme".into(), "x-api-key".into());
    object.insert("modelDiscoveryEnabled".into(), false.into());
    object.remove("anthropicBaseUrl");
    object.remove("anthropicApiKey");
    object.insert(
        "inferenceModels".into(),
        Value::Array(
            MODELS
                .iter()
                .map(|model| {
                    json!({
                        "name": model.desktop_alias,
                        "labelOverride": model.desktop_label,
                        "supports1m": model.context.is_some_and(|context| context >= 1_000_000),
                    })
                })
                .collect(),
        ),
    );
    write_json(path, &config)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn merge_preserves_verified_gateway_contract() {
        let dir = std::env::temp_dir().join(format!(
            "auranion-claude-desktop-contract-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("config.json");
        write_json(
            &path,
            &json!({
                "autoModeEnabled": true,
                "anthropicBaseUrl": "https://obsolete.example/v1",
                "anthropicApiKey": "obsolete-key",
                "inferenceGatewayBaseUrl": "http://127.0.0.1:8787",
                "inferenceGatewayApiKey": "local-auranion-proxy",
                "inferenceGatewayAuthScheme": "bearer"
            }),
        )
        .unwrap();

        merge(&path, "test-key").unwrap();
        let config = read_json(&path).unwrap();
        assert_eq!(config.get("autoModeEnabled"), Some(&Value::Bool(true)));
        assert_eq!(
            config.get("inferenceProvider").and_then(Value::as_str),
            Some("gateway")
        );
        assert_eq!(
            config
                .get("inferenceCredentialKind")
                .and_then(Value::as_str),
            Some("static")
        );
        assert_eq!(
            config
                .get("inferenceGatewayBaseUrl")
                .and_then(Value::as_str),
            Some(BASE_URL)
        );
        assert_eq!(
            config.get("inferenceGatewayApiKey").and_then(Value::as_str),
            Some("test-key")
        );
        assert_eq!(
            config
                .get("inferenceGatewayAuthScheme")
                .and_then(Value::as_str),
            Some("x-api-key")
        );
        assert_eq!(
            config.get("modelDiscoveryEnabled"),
            Some(&Value::Bool(false))
        );
        assert!(config.get("anthropicBaseUrl").is_none());
        assert!(config.get("anthropicApiKey").is_none());

        let models = config
            .get("inferenceModels")
            .and_then(Value::as_array)
            .unwrap();
        assert_eq!(models.len(), MODELS.len());
        assert_eq!(
            models[0].get("name").and_then(Value::as_str),
            Some("claude-opus-4-8")
        );
        assert_eq!(
            models[5].get("name").and_then(Value::as_str),
            Some("claude-haiku-4-5-20251001")
        );

        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn merge_rejects_non_object_root() {
        let path = std::env::temp_dir().join(format!(
            "auranion-claude-desktop-non-object-{}.json",
            std::process::id()
        ));
        std::fs::write(&path, "[]").unwrap();
        assert!(merge(&path, "key").is_err());
        std::fs::remove_file(path).unwrap();
    }
}
