use anyhow::{Context, Result};
use directories::BaseDirs;
use serde_yaml::Value as YamlValue;
use std::{
    fs,
    path::{Path, PathBuf},
};

use crate::catalog::MODELS;

use super::super::{
    BASE_URL,
    io::{strip_bom, write_bytes},
    state::State,
};

const PROVIDER_KEY: &str = "auranion";
const LEGACY_PROVIDER_NAMES: [&str; 2] = ["auranion", "Auranion"];

pub(super) fn detect(dirs: &BaseDirs) -> bool {
    hermes_home(dirs).join("config.yaml").exists()
}

pub(super) fn diagnostics(dirs: &BaseDirs) -> Vec<String> {
    let path = hermes_home(dirs).join("config.yaml");
    if path.exists() {
        Vec::new()
    } else {
        vec!["hermes config.yaml not found".into()]
    }
}

pub(super) fn select(
    dirs: &BaseDirs,
    data_dir: &Path,
    state: &mut State,
    api_key: &str,
) -> Result<()> {
    let path = hermes_home(dirs).join("config.yaml");
    state.backup(data_dir, &path)?;
    merge(&path, api_key)
}

pub(super) fn deselect(dirs: &BaseDirs, state: &mut State) -> Result<()> {
    let path = hermes_home(dirs).join("config.yaml");
    if !path.exists() {
        state.forget_baseline(&path);
        return Ok(());
    }
    if state.baseline_for(&path).is_none() {
        remove_provider(&path)?;
        return Ok(());
    }
    restore(&path, state)
}

fn hermes_home(dirs: &BaseDirs) -> PathBuf {
    if let Ok(val) = std::env::var("HERMES_HOME") {
        let trimmed = val.trim();
        if !trimmed.is_empty() {
            return PathBuf::from(trimmed);
        }
    }
    if cfg!(windows) {
        if let Ok(val) = std::env::var("LOCALAPPDATA") {
            let trimmed = val.trim();
            if !trimmed.is_empty() {
                return PathBuf::from(trimmed).join("hermes");
            }
        }
        dirs.data_local_dir().join("hermes")
    } else {
        dirs.home_dir().join(".hermes")
    }
}

fn merge(path: &Path, api_key: &str) -> Result<()> {
    let mut root = read_yaml(path)?;
    if !root.is_mapping() {
        anyhow::bail!("hermes config root must be a mapping");
    }
    let provider = auranion_provider(api_key);
    upsert_provider(&mut root, provider);
    remove_legacy_custom_providers(&mut root);
    migrate_tier_default(&mut root);
    write_yaml(path, &root)
}

fn migrate_tier_default(root: &mut YamlValue) {
    let Some(model) = root.get_mut("model").and_then(YamlValue::as_mapping_mut) else {
        return;
    };
    let provider = model.get("provider").and_then(YamlValue::as_str);
    let is_auranion = match provider {
        Some("auranion" | "Auranion" | "custom:auranion") => true,
        None | Some("custom" | "auto") => model
            .get("base_url")
            .and_then(YamlValue::as_str)
            .is_some_and(|url| url.trim_end_matches('/') == BASE_URL),
        _ => false,
    };
    if !is_auranion {
        return;
    }
    // ponytail: migrate only retired tier defaults; leave other provider IDs alone.
    if let Some(default) = model.get_mut("default") {
        if let Some(tier) = default
            .as_str()
            .and_then(|id| id.strip_prefix("auranion/"))
            .filter(|id| MODELS.iter().any(|model| model.upstream == *id))
        {
            *default = YamlValue::String(tier.into());
        }
    }
}

fn remove_provider(path: &Path) -> Result<()> {
    let mut root = read_yaml(path)?;
    if !root.is_mapping() {
        return Ok(());
    }
    let mut changed = false;
    if remove_provider_entry(&mut root) {
        changed = true;
    }
    if remove_legacy_custom_providers(&mut root) {
        changed = true;
    }
    if changed {
        write_yaml(path, &root)?;
    }
    Ok(())
}

fn restore(path: &Path, state: &State) -> Result<()> {
    let Some(baseline) = state.baseline_for(path) else {
        return Ok(());
    };
    if !path.exists() {
        return Ok(());
    }
    let existed = state.baseline_existed(path).unwrap_or(true);
    if !existed {
        fs::remove_file(path).with_context(|| format!("remove {}", path.display()))?;
        return Ok(());
    }
    let baseline_text =
        fs::read_to_string(&baseline).with_context(|| format!("read {}", baseline.display()))?;
    let original =
        parse_yaml_text(&baseline_text).with_context(|| format!("parse {}", baseline.display()))?;
    let mut current = read_yaml(path)?;
    if !current.is_mapping() {
        current = YamlValue::Mapping(Default::default());
    }
    restore_provider(&mut current, &original);
    write_yaml(path, &current)
}

fn restore_provider(current: &mut YamlValue, original: &YamlValue) {
    let original_provider = provider_entry(original);
    if let Some(provider) = original_provider {
        upsert_provider(current, provider.clone());
    } else {
        remove_provider_entry(current);
    }

    let original_legacy = legacy_entries(original);
    let current_non_legacy = current
        .as_mapping()
        .and_then(|m| m.get(YamlValue::String("custom_providers".into())))
        .and_then(|v| v.as_sequence())
        .map(|seq| {
            seq.iter()
                .filter(|entry| {
                    entry
                        .as_mapping()
                        .and_then(|m| m.get(YamlValue::String("name".into())))
                        .and_then(|v| v.as_str())
                        .is_none_or(|name| !LEGACY_PROVIDER_NAMES.contains(&name))
                })
                .cloned()
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();

    let merged = if original_legacy.is_empty() {
        current_non_legacy
    } else {
        let mut v = current_non_legacy;
        v.extend(original_legacy);
        v
    };

    if let Some(map) = current.as_mapping_mut() {
        if merged.is_empty() {
            map.remove(YamlValue::String("custom_providers".into()));
        } else {
            map.insert(
                YamlValue::String("custom_providers".into()),
                YamlValue::Sequence(merged),
            );
        }
    }
}

fn provider_entry(value: &YamlValue) -> Option<YamlValue> {
    value
        .as_mapping()?
        .get(YamlValue::String("providers".into()))?
        .as_mapping()?
        .get(YamlValue::String(PROVIDER_KEY.into()))
        .cloned()
}

fn legacy_entries(value: &YamlValue) -> Vec<YamlValue> {
    let Some(seq) = value
        .as_mapping()
        .and_then(|m| m.get(YamlValue::String("custom_providers".into())))
        .and_then(|v| v.as_sequence())
    else {
        return Vec::new();
    };
    seq.iter()
        .filter(|entry| {
            entry
                .as_mapping()
                .and_then(|m| m.get(YamlValue::String("name".into())))
                .and_then(|v| v.as_str())
                .is_some_and(|name| LEGACY_PROVIDER_NAMES.contains(&name))
        })
        .cloned()
        .collect()
}

fn upsert_provider(root: &mut YamlValue, provider: YamlValue) {
    let map = root.as_mapping_mut().expect("caller ensures mapping");
    let key = YamlValue::String("providers".into());
    if map.get(&key).is_none_or(|value| !value.is_mapping()) {
        map.insert(key.clone(), YamlValue::Mapping(Default::default()));
    }
    let providers = map.get_mut(&key).expect("just inserted mapping");
    let providers_map = providers.as_mapping_mut().expect("providers is mapping");
    providers_map.insert(YamlValue::String(PROVIDER_KEY.into()), provider);
}

fn remove_provider_entry(root: &mut YamlValue) -> bool {
    let Some(map) = root.as_mapping_mut() else {
        return false;
    };
    let Some(providers) = map.get_mut(YamlValue::String("providers".into())) else {
        return false;
    };
    let Some(providers_map) = providers.as_mapping_mut() else {
        return false;
    };
    let removed = providers_map
        .remove(YamlValue::String(PROVIDER_KEY.into()))
        .is_some();
    if providers_map.is_empty() {
        map.remove(YamlValue::String("providers".into()));
    }
    removed
}

fn remove_legacy_custom_providers(root: &mut YamlValue) -> bool {
    let Some(map) = root.as_mapping_mut() else {
        return false;
    };
    let key = YamlValue::String("custom_providers".into());
    let (before, after, empty) = {
        let Some(seq) = map.get_mut(&key).and_then(|v| v.as_sequence_mut()) else {
            return false;
        };
        let before = seq.len();
        seq.retain(|entry| {
            entry
                .as_mapping()
                .and_then(|m| m.get(YamlValue::String("name".into())))
                .and_then(|v| v.as_str())
                .is_none_or(|name| !LEGACY_PROVIDER_NAMES.contains(&name))
        });
        (before, seq.len(), seq.is_empty())
    };
    if empty {
        map.remove(key);
    }
    after != before
}

fn auranion_provider(api_key: &str) -> YamlValue {
    let mut map = serde_yaml::Mapping::new();
    map.insert(
        YamlValue::String("base_url".into()),
        YamlValue::String(BASE_URL.into()),
    );
    map.insert(
        YamlValue::String("api_key".into()),
        YamlValue::String(api_key.into()),
    );
    map.insert(
        YamlValue::String("api_mode".into()),
        YamlValue::String("chat_completions".into()),
    );
    map.insert("discover_models".into(), YamlValue::Bool(false));
    let mut models = serde_yaml::Mapping::new();
    for model in MODELS {
        let mut meta = serde_yaml::Mapping::new();
        if let Some(ctx) = model.context {
            meta.insert(
                YamlValue::String("context_length".into()),
                YamlValue::Number(serde_yaml::Number::from(ctx)),
            );
        }
        if model.reasoning {
            meta.insert(YamlValue::String("reasoning".into()), YamlValue::Bool(true));
        }
        models.insert(
            YamlValue::String(model.upstream.into()),
            YamlValue::Mapping(meta),
        );
    }
    map.insert(
        YamlValue::String("models".into()),
        YamlValue::Mapping(models),
    );
    YamlValue::Mapping(map)
}

fn read_yaml(path: &Path) -> Result<YamlValue> {
    if !path.exists() {
        return Ok(YamlValue::Mapping(Default::default()));
    }
    let text = fs::read_to_string(path).with_context(|| format!("read {}", path.display()))?;
    parse_yaml_text(&text).with_context(|| format!("parse {}", path.display()))
}

fn parse_yaml_text(text: &str) -> Result<YamlValue> {
    if text.trim().is_empty() {
        return Ok(YamlValue::Mapping(Default::default()));
    }
    let stripped = strip_bom(text);
    let value: YamlValue = serde_yaml::from_str(stripped).context("parse yaml")?;
    normalize_yaml_value(value)
}

fn normalize_yaml_value(value: YamlValue) -> Result<YamlValue> {
    let json = serde_json::to_value(&value).context("convert yaml to json")?;
    let yaml = serde_yaml::to_value(&json).context("convert json to yaml")?;
    Ok(yaml)
}

fn write_yaml(path: &Path, value: &YamlValue) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let text = serde_yaml::to_string(value).context("serialize yaml")?;
    write_bytes(path, text.as_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn tmp_path(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!("auranion-hermes-{name}-{}", std::process::id()))
    }

    #[test]
    fn merge_is_idempotent_and_preserves_other_providers() {
        let dir = tmp_path("idempotent");
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("config.yaml");
        fs::write(
            &path,
            r#"providers:
  other:
    base_url: https://other.example.com/v1
    api_key: other-key
"#,
        )
        .unwrap();

        merge(&path, "test-key").unwrap();
        let first = read_yaml(&path).unwrap();
        merge(&path, "test-key").unwrap();
        let second = read_yaml(&path).unwrap();
        assert_eq!(second, first);

        let root = read_yaml(&path).unwrap();
        let providers = root
            .as_mapping()
            .unwrap()
            .get(YamlValue::String("providers".into()))
            .unwrap()
            .as_mapping()
            .unwrap();
        assert!(providers.contains_key(YamlValue::String("other".into())));
        let auranion = providers
            .get(YamlValue::String(PROVIDER_KEY.into()))
            .unwrap()
            .as_mapping()
            .unwrap();
        assert_eq!(
            auranion
                .get(YamlValue::String("base_url".into()))
                .and_then(|v| v.as_str()),
            Some(BASE_URL)
        );
        assert_eq!(
            auranion
                .get(YamlValue::String("api_key".into()))
                .and_then(|v| v.as_str()),
            Some("test-key")
        );
        assert!(
            auranion
                .get(YamlValue::String("models".into()))
                .and_then(|v| v.as_mapping())
                .is_some_and(|m| m.len() == MODELS.len())
        );

        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn merge_removes_legacy_custom_providers() {
        let dir = tmp_path("legacy");
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("config.yaml");
        fs::write(
            &path,
            r#"custom_providers:
  - name: auranion
    base_url: https://agent.auranion.com/v1
    api_key: old
  - name: other
    base_url: https://other.example.com/v1
"#,
        )
        .unwrap();

        merge(&path, "new-key").unwrap();
        let root = read_yaml(&path).unwrap();
        let seq = root
            .as_mapping()
            .unwrap()
            .get(YamlValue::String("custom_providers".into()))
            .and_then(|v| v.as_sequence())
            .unwrap();
        assert_eq!(seq.len(), 1);
        assert_eq!(
            seq[0]
                .as_mapping()
                .unwrap()
                .get(YamlValue::String("name".into()))
                .and_then(|v| v.as_str()),
            Some("other")
        );

        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn remove_provider_preserves_other_providers() {
        let dir = tmp_path("remove");
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("config.yaml");
        fs::write(
            &path,
            r#"providers:
  other:
    base_url: https://other.example.com/v1
  auranion:
    base_url: https://agent.auranion.com/v1
    api_key: key
"#,
        )
        .unwrap();

        remove_provider(&path).unwrap();
        let root = read_yaml(&path).unwrap();
        let providers = root
            .as_mapping()
            .unwrap()
            .get(YamlValue::String("providers".into()))
            .unwrap()
            .as_mapping()
            .unwrap();
        assert!(!providers.contains_key(YamlValue::String(PROVIDER_KEY.into())));
        assert!(providers.contains_key(YamlValue::String("other".into())));

        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn merge_replaces_retired_upstream_ids_with_tiers() {
        let dir = tmp_path("retired");
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("config.yaml");
        fs::write(
            &path,
            r#"providers:
  auranion:
    base_url: https://agent.auranion.com/v1
    api_key: old
    discover_models: true
    models:
      cx/gpt-6-astra: {}
      deepseek/deepseek-v4.1-flash: {}
"#,
        )
        .unwrap();

        merge(&path, "new-key").unwrap();
        let root = read_yaml(&path).unwrap();
        assert_eq!(root["providers"]["auranion"]["discover_models"], false);
        let models = root
            .as_mapping()
            .unwrap()
            .get(YamlValue::String("providers".into()))
            .unwrap()
            .as_mapping()
            .unwrap()
            .get(YamlValue::String(PROVIDER_KEY.into()))
            .unwrap()
            .as_mapping()
            .unwrap()
            .get(YamlValue::String("models".into()))
            .unwrap()
            .as_mapping()
            .unwrap();
        let keys: Vec<_> = models.keys().filter_map(|k| k.as_str()).collect();
        assert_eq!(keys, ["gigachad", "chad", "sigma", "alpha"]);
        for metadata in models.values() {
            assert_eq!(metadata["context_length"].as_u64(), Some(256_000));
        }

        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn merge_migrates_tier_defaults_only_for_auranion() {
        let dir = tmp_path("tier-defaults");
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("config.yaml");
        for (provider, base_url, migrate) in [
            (Some("auranion"), None, true),
            (Some("Auranion"), None, true),
            (Some("custom:auranion"), None, true),
            (Some("custom"), Some(BASE_URL), true),
            (Some("auto"), Some("https://agent.auranion.com/v1/"), true),
            (None, Some(BASE_URL), true),
            (Some("other"), Some(BASE_URL), false),
            (Some("custom"), Some("https://other.example.com/v1"), false),
            (None, None, false),
        ] {
            for tier in ["gigachad", "chad", "sigma", "alpha", "private-model"] {
                let mut root = serde_yaml::to_value(serde_json::json!({
                    "model": { "default": format!("auranion/{tier}"), "context_length": 32000 },
                    "display": { "compact": true }
                }))
                .unwrap();
                let model = root["model"].as_mapping_mut().unwrap();
                if let Some(provider) = provider {
                    model.insert("provider".into(), provider.into());
                }
                if let Some(base_url) = base_url {
                    model.insert("base_url".into(), base_url.into());
                }
                let mut expected = root["model"].clone();
                if migrate && tier != "private-model" {
                    expected["default"] = tier.into();
                }
                write_yaml(&path, &root).unwrap();
                merge(&path, "test-key").unwrap();
                let actual = read_yaml(&path).unwrap();
                assert_eq!(
                    actual["model"], expected,
                    "provider={provider:?}, tier={tier}"
                );
                assert_eq!(actual["display"], root["display"]);
                merge(&path, "test-key").unwrap();
                assert_eq!(read_yaml(&path).unwrap(), actual);
            }
        }
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn merge_rejects_non_mapping_root() {
        let dir = tmp_path("non-mapping");
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("config.yaml");
        fs::write(&path, "[]\n").unwrap();
        let result = merge(&path, "key");
        assert!(result.is_err());
        fs::remove_dir_all(&dir).unwrap();
    }
}
