use anyhow::{Context, Result};
use directories::BaseDirs;
use serde_json::{Value, json};
use std::{
    fs,
    path::{Path, PathBuf},
};

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

/// Deterministic profile used when `_meta.json` does not yet exist or has no
/// `appliedId`. Mirrors cc-switch's fixed-profile approach so a fresh macOS
/// install (where Claude Desktop has not created a profile yet) still works.
const OWNED_PROFILE_ID: &str = "00000000-0000-4000-8000-0000a3cade00";
const OWNED_PROFILE_NAME: &str = "Auranion";

const DEPLOYMENT_MODE_KEY: &str = "deploymentMode";
const DEPLOYMENT_MODE_3P: &str = "3p";

pub(super) fn detect(dirs: &BaseDirs) -> bool {
    config_root(dirs).is_dir()
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
    select_at_root(&config_root(dirs), data_dir, state, api_key)
}

fn select_at_root(root: &Path, data_dir: &Path, state: &mut State, api_key: &str) -> Result<()> {
    let (profile_path, owned) = resolve_profile_path(root)?;
    let meta_path = metadata_path(root);
    let consumer_config = consumer_config_path(root);

    // The verified appliedId flow (a configured app, e.g. the working Windows
    // setup) writes only the profile file. The owned fallback (fresh install
    // where Claude Desktop has not created a profile yet) additionally
    // manages `_meta.json` and `deploymentMode`, mirroring cc-switch.
    state.backup(data_dir, &profile_path)?;
    if owned {
        state.backup(data_dir, &meta_path)?;
        state.backup(data_dir, &consumer_config)?;
    }

    merge(&profile_path, api_key)?;
    if owned {
        write_meta(&meta_path)?;
        write_deployment_mode(&consumer_config)?;
    }

    state.desktop_config = Some(profile_path);
    state.desktop_owned_meta = owned.then_some(meta_path);
    state.desktop_consumer_config = owned.then_some(consumer_config);
    state.startup_artifact = None;
    Ok(())
}

pub(super) fn deselect(state: &mut State) -> Result<()> {
    let Some(path) = state.desktop_config.clone() else {
        return Ok(());
    };

    if state.baseline_existed(&path).unwrap_or(false) {
        restore_json_fields(&path, state, |current, original| {
            for key in OWNED_KEYS {
                restore_key(current, original, key)?;
            }
            Ok(())
        })?;
    } else if path.exists() {
        fs::remove_file(&path).with_context(|| format!("remove {}", path.display()))?;
    }
    state.forget_baseline(&path);

    if let Some(meta) = state.desktop_owned_meta.take() {
        if state.baseline_existed(&meta).unwrap_or(false) {
            restore_json_fields(&meta, state, |current, original| {
                for key in ["appliedId", "entries"] {
                    restore_key(current, original, key)?;
                }
                Ok(())
            })?;
        } else if meta.exists() {
            std::fs::remove_file(&meta).with_context(|| format!("remove {}", meta.display()))?;
        }
        state.forget_baseline(&meta);
    }

    if let Some(consumer) = state.desktop_consumer_config.take() {
        if state.baseline_existed(&consumer).unwrap_or(false) {
            restore_json_fields(&consumer, state, |current, original| {
                restore_key(current, original, DEPLOYMENT_MODE_KEY)
            })?;
        } else if consumer.exists() {
            std::fs::remove_file(&consumer)
                .with_context(|| format!("remove {}", consumer.display()))?;
        }
        state.forget_baseline(&consumer);
    }

    state.desktop_config = None;
    state.startup_artifact = None;
    Ok(())
}

fn metadata_path(root: &Path) -> PathBuf {
    root.join("configLibrary").join("_meta.json")
}

/// Returns `(profile_path, owned)` where `owned` is true only when falling back
/// to the deterministic Auranion profile because `_meta.json` has no valid
/// `appliedId`. When `_meta.json` exists with a valid `appliedId`, the app's own
/// profile is used and no extra files are owned — this is the verified working
/// Windows path.
fn resolve_profile_path(root: &Path) -> Result<(PathBuf, bool)> {
    let meta_path = metadata_path(root);
    let metadata = read_json(&meta_path)?;
    if let Some(applied_id) = metadata
        .get("appliedId")
        .and_then(Value::as_str)
        .filter(|id| !id.is_empty() && !id.contains(['/', '\\', ':']))
    {
        return Ok((
            root.join("configLibrary")
                .join(format!("{applied_id}.json")),
            false,
        ));
    }
    Ok((
        root.join("configLibrary")
            .join(format!("{OWNED_PROFILE_ID}.json")),
        true,
    ))
}

fn consumer_config_path(root: &Path) -> PathBuf {
    root.join("claude_desktop_config.json")
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

fn write_meta(path: &Path) -> Result<()> {
    let mut value = read_json(path)?;
    let object = json_object_mut(&mut value, "_meta.json root")?;
    object.insert("appliedId".into(), OWNED_PROFILE_ID.into());
    let mut entries = object
        .get("entries")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    entries.retain(|entry| entry.get("id").and_then(Value::as_str) != Some(OWNED_PROFILE_ID));
    entries.push(json!({
        "id": OWNED_PROFILE_ID,
        "name": OWNED_PROFILE_NAME,
    }));
    object.insert("entries".into(), Value::Array(entries));
    write_json(path, &value)
}

fn write_deployment_mode(path: &Path) -> Result<()> {
    let mut value = read_json(path)?;
    let object = json_object_mut(&mut value, "claude_desktop_config.json root")?;
    object.insert(
        DEPLOYMENT_MODE_KEY.into(),
        Value::String(DEPLOYMENT_MODE_3P.into()),
    );
    write_json(path, &value)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_root(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "auranion-claude-desktop-{name}-{}",
            std::process::id()
        ))
    }

    fn library(root: &Path) -> PathBuf {
        root.join("configLibrary")
    }

    #[test]
    fn merge_preserves_verified_gateway_contract() {
        let dir = temp_root("contract");
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

    #[test]
    fn write_meta_sets_applied_id_and_owned_entry() {
        let dir = temp_root("write-meta");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("_meta.json");
        write_json(
            &path,
            &json!({
                "appliedId": "other-profile",
                "entries": [{"id": "other-profile", "name": "Someone"}]
            }),
        )
        .unwrap();

        write_meta(&path).unwrap();
        let meta = read_json(&path).unwrap();
        assert_eq!(
            meta.get("appliedId").and_then(Value::as_str),
            Some(OWNED_PROFILE_ID)
        );
        let entries = meta.get("entries").and_then(Value::as_array).unwrap();
        assert_eq!(entries.len(), 2);
        assert!(
            entries
                .iter()
                .any(|entry| { entry.get("id").and_then(Value::as_str) == Some(OWNED_PROFILE_ID) })
        );

        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn write_meta_handles_missing_file() {
        let dir = temp_root("write-meta-missing");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("_meta.json");

        write_meta(&path).unwrap();
        let meta = read_json(&path).unwrap();
        assert_eq!(
            meta.get("appliedId").and_then(Value::as_str),
            Some(OWNED_PROFILE_ID)
        );

        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn write_deployment_mode_sets_3p() {
        let dir = temp_root("deploy-mode");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("claude_desktop_config.json");
        write_json(&path, &json!({"foo": "bar"})).unwrap();

        write_deployment_mode(&path).unwrap();
        let value = read_json(&path).unwrap();
        assert_eq!(
            value.get("deploymentMode").and_then(Value::as_str),
            Some("3p")
        );
        assert_eq!(value.get("foo").and_then(Value::as_str), Some("bar"));

        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn resolve_uses_existing_applied_id_without_owning_files() {
        let root = temp_root("resolve-existing");
        let _ = std::fs::remove_dir_all(&root);
        fs::create_dir_all(library(&root)).unwrap();
        write_json(
            &metadata_path(&root),
            &json!({"appliedId": "abc-123", "entries": []}),
        )
        .unwrap();

        let (path, owned) = resolve_profile_path(&root).unwrap();
        assert_eq!(path, library(&root).join("abc-123.json"));
        assert!(!owned);

        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn resolve_falls_back_to_owned_profile_when_meta_missing_or_invalid() {
        let root = temp_root("resolve-owned");
        let _ = std::fs::remove_dir_all(&root);
        fs::create_dir_all(library(&root)).unwrap();

        // Missing _meta.json
        let (path, owned) = resolve_profile_path(&root).unwrap();
        assert_eq!(
            path,
            library(&root).join(format!("{OWNED_PROFILE_ID}.json"))
        );
        assert!(owned);

        // Invalid appliedId (path traversal / empty)
        write_json(&metadata_path(&root), &json!({"appliedId": "../evil"})).unwrap();
        let (path, owned) = resolve_profile_path(&root).unwrap();
        assert_eq!(
            path,
            library(&root).join(format!("{OWNED_PROFILE_ID}.json"))
        );
        assert!(owned);

        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn select_on_fresh_install_then_deselect_restores_everything() {
        let root = temp_root("select-fresh");
        let _ = std::fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        let data_dir = temp_root("select-fresh-data");
        let _ = std::fs::remove_dir_all(&data_dir);
        fs::create_dir_all(&data_dir).unwrap();
        let mut state = State::default();

        select_at_root(&root, &data_dir, &mut state, "fresh-key").unwrap();

        let profile = library(&root).join(format!("{OWNED_PROFILE_ID}.json"));
        assert!(profile.exists());
        let config = read_json(&profile).unwrap();
        assert_eq!(config["inferenceGatewayApiKey"], json!("fresh-key"));
        assert_eq!(config["inferenceProvider"], json!("gateway"));
        assert_eq!(
            config["inferenceModels"].as_array().unwrap().len(),
            MODELS.len()
        );
        assert_eq!(
            read_json(&metadata_path(&root)).unwrap()["appliedId"],
            json!(OWNED_PROFILE_ID)
        );
        assert_eq!(
            read_json(&consumer_config_path(&root)).unwrap()["deploymentMode"],
            json!("3p")
        );

        deselect(&mut state).unwrap();

        // All owned files were created fresh, so deselect removes them.
        assert!(!profile.exists());
        assert!(!metadata_path(&root).exists());
        assert!(!consumer_config_path(&root).exists());
        assert!(state.desktop_config.is_none());
        assert!(state.desktop_owned_meta.is_none());
        assert!(state.desktop_consumer_config.is_none());

        fs::remove_dir_all(&root).unwrap();
        fs::remove_dir_all(&data_dir).unwrap();
    }

    #[test]
    fn select_on_fresh_install_preserves_preexisting_meta() {
        let root = temp_root("select-fresh-meta");
        let _ = std::fs::remove_dir_all(&root);
        fs::create_dir_all(library(&root)).unwrap();
        // _meta.json exists but has no appliedId (fresh/partial install).
        write_json(&metadata_path(&root), &json!({"entries": []})).unwrap();
        let consumer = consumer_config_path(&root);
        write_json(&consumer, &json!({"foo": "bar"})).unwrap();
        let data_dir = temp_root("select-fresh-meta-data");
        let _ = std::fs::remove_dir_all(&data_dir);
        fs::create_dir_all(&data_dir).unwrap();
        let mut state = State::default();

        select_at_root(&root, &data_dir, &mut state, "fresh-key").unwrap();

        assert_eq!(
            read_json(&metadata_path(&root)).unwrap()["appliedId"],
            json!(OWNED_PROFILE_ID)
        );
        // Preexisting consumer key preserved alongside deploymentMode.
        let value = read_json(&consumer).unwrap();
        assert_eq!(value["deploymentMode"], json!("3p"));
        assert_eq!(value["foo"], json!("bar"));

        deselect(&mut state).unwrap();

        // _meta.json and consumer existed before → restored, not deleted.
        assert_eq!(
            read_json(&metadata_path(&root)).unwrap(),
            json!({"entries": []})
        );
        assert_eq!(read_json(&consumer).unwrap(), json!({"foo": "bar"}));

        fs::remove_dir_all(&root).unwrap();
        fs::remove_dir_all(&data_dir).unwrap();
    }

    #[test]
    fn select_with_existing_applied_id_writes_only_the_profile() {
        let root = temp_root("select-existing");
        let _ = std::fs::remove_dir_all(&root);
        fs::create_dir_all(library(&root)).unwrap();
        write_json(
            &metadata_path(&root),
            &json!({"appliedId": "win-42", "entries": []}),
        )
        .unwrap();
        let data_dir = temp_root("select-existing-data");
        let _ = std::fs::remove_dir_all(&data_dir);
        fs::create_dir_all(&data_dir).unwrap();
        let mut state = State::default();

        select_at_root(&root, &data_dir, &mut state, "win-key").unwrap();

        let profile = library(&root).join("win-42.json");
        assert!(profile.exists());
        assert_eq!(
            read_json(&profile).unwrap()["inferenceGatewayApiKey"],
            json!("win-key")
        );

        // Verified Windows contract: no _meta.json / deploymentMode writes.
        assert_eq!(
            read_json(&metadata_path(&root)).unwrap()["appliedId"],
            json!("win-42")
        );
        assert!(!consumer_config_path(&root).exists());
        assert!(state.desktop_owned_meta.is_none());
        assert!(state.desktop_consumer_config.is_none());

        deselect(&mut state).unwrap();
        assert!(!profile.exists());

        fs::remove_dir_all(&root).unwrap();
        fs::remove_dir_all(&data_dir).unwrap();
    }
}
