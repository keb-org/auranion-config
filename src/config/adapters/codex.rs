use anyhow::{Context, Result, bail};
use directories::BaseDirs;
use serde_json::{Map, Value, json};
use std::{
    fs,
    path::{Path, PathBuf},
};
use toml_edit::{Array, DocumentMut, Item, Table, TableLike, value};

use crate::catalog::{DEFAULT_MODEL, MODELS};

use super::super::{
    BASE_URL,
    integration::Integration,
    io::{read_json, read_toml, write_bytes},
    state::State,
};

#[cfg(test)]
use super::super::io::{write_json, write_toml};

const OWNED_ROOT_KEYS: [&str; 3] = ["model", "model_provider", "model_catalog_json"];
const CLI_ROOT_KEYS: [&str; 2] = ["model", "model_provider"];
const OWNED_PROVIDER_KEYS: [&str; 8] = [
    "name",
    "base_url",
    "env_key",
    "env_key_instructions",
    "experimental_bearer_token",
    "wire_api",
    "requires_openai_auth",
    "supports_websockets",
];
const OWNED_AUTH_KEYS: [&str; 4] = ["command", "args", "timeout_ms", "refresh_interval_ms"];
const CODEX_REASONING_EFFORTS: [&str; 6] = ["none", "minimal", "low", "medium", "high", "xhigh"];
const BASE_INSTRUCTIONS: &str =
    "You are Codex, an agent working through the Auranion model gateway.";
const TOKEN_COMMAND: &str = "provider-token";
const LEGACY_ROOT_KEYS: [&str; 4] = [
    "preferred_auth_method",
    "model",
    "model_provider",
    "model_catalog_json",
];
const LEGACY_SUBAGENT_KEYS: [&str; 2] = ["description", "model"];

pub(super) fn detect(dirs: &BaseDirs) -> bool {
    let Ok(home) = codex_home(dirs) else {
        return false;
    };
    home.join("config.toml").exists() || home.join("auth.json").exists()
}

pub(super) fn diagnostics(dirs: &BaseDirs) -> Vec<String> {
    match codex_home(dirs) {
        Ok(home) => (!home.join("config.toml").exists())
            .then_some("config missing".into())
            .into_iter()
            .collect(),
        Err(_) => vec!["CODEX_HOME invalid".into()],
    }
}

pub(super) fn desktop_diagnostics(dirs: &BaseDirs) -> Vec<String> {
    match codex_home(dirs) {
        Ok(home) => (!home.join("desktop-model-providers.json").exists())
            .then_some("Desktop provider map missing".into())
            .into_iter()
            .collect(),
        Err(_) => vec!["CODEX_HOME invalid".into()],
    }
}

pub(super) fn reconcile(
    dirs: &BaseDirs,
    data_dir: &Path,
    state: &mut State,
    wanted: &[Integration],
    previous_api_key: Option<&str>,
) -> Result<()> {
    let desktop = wanted.contains(&Integration::CodexDesktop);
    let cli = wanted.contains(&Integration::CodexCli);
    let configured = configured_codex_home(dirs, state)?;
    if desktop || cli {
        let home = codex_home(dirs)?;
        reconcile_at_home(
            &home,
            &configured,
            data_dir,
            state,
            desktop,
            cli,
            previous_api_key,
        )
    } else if state.codex_home.is_some() {
        deselect_at_home(&configured, data_dir, state, previous_api_key)
    } else {
        Ok(())
    }
}

#[cfg(test)]
fn select_at_home(
    home: &Path,
    previous_home: &Path,
    data_dir: &Path,
    state: &mut State,
    previous_api_key: Option<&str>,
) -> Result<()> {
    reconcile_at_home(
        home,
        previous_home,
        data_dir,
        state,
        false,
        true,
        previous_api_key,
    )
}

fn reconcile_at_home(
    home: &Path,
    previous_home: &Path,
    data_dir: &Path,
    state: &mut State,
    desktop: bool,
    cli: bool,
    previous_api_key: Option<&str>,
) -> Result<()> {
    let previous = (previous_home != home)
        .then(|| restore_home_plan(previous_home, state, previous_api_key))
        .transpose()?;
    let target = select_plan(home, state, previous_api_key, desktop, cli)?;

    let mut writes = previous.into_iter().flatten().collect::<Vec<_>>();
    writes.extend(target.writes);
    let paths = writes
        .iter()
        .map(|write| write.path.as_path())
        .collect::<Vec<_>>();
    state.begin_codex_transaction(data_dir, &paths)?;
    state.set_codex_transaction_selection(
        Some(home.to_path_buf()),
        &[
            desktop.then_some(Integration::CodexDesktop),
            cli.then_some(Integration::CodexCli),
        ]
        .into_iter()
        .flatten()
        .collect::<Vec<_>>(),
    );
    state.backup(data_dir, &target.config)?;
    state.backup(data_dir, &target.catalog)?;
    if target.desktop_contents.is_some() || target.restore_desktop_providers {
        state.backup(data_dir, &target.desktop_providers)?;
    }
    for write in &writes {
        record_write_expected(state, data_dir, write)?;
    }
    state.record_generated_bytes(data_dir, &target.config, &target.config_contents)?;
    if let Some(catalog_contents) = &target.catalog_contents {
        state.record_generated_bytes(data_dir, &target.catalog, catalog_contents)?;
    }
    if let Some(desktop_contents) = &target.desktop_contents {
        state.record_generated_bytes(data_dir, &target.desktop_providers, desktop_contents)?;
    } else if target.restore_desktop_providers {
        state.forget_baseline(&target.desktop_providers);
        state.forget_generated(&target.desktop_providers);
    }
    if previous_home != home {
        forget_restored_home_state(previous_home, state);
    }
    state.codex_home = Some(home.to_path_buf());
    state.save(data_dir)?;
    apply_transaction_writes(data_dir, state, writes)
}

fn deselect_at_home(
    home: &Path,
    data_dir: &Path,
    state: &mut State,
    previous_api_key: Option<&str>,
) -> Result<()> {
    let writes = restore_home_plan(home, state, previous_api_key)?;
    let paths = writes
        .iter()
        .map(|write| write.path.as_path())
        .collect::<Vec<_>>();
    state.begin_codex_transaction(data_dir, &paths)?;
    for write in &writes {
        record_write_expected(state, data_dir, write)?;
    }
    forget_restored_home_state(home, state);
    state.codex_home = None;
    state.save(data_dir)?;
    apply_transaction_writes(data_dir, state, writes)
}

fn configured_codex_home(dirs: &BaseDirs, state: &State) -> Result<PathBuf> {
    Ok(state
        .codex_home
        .clone()
        .unwrap_or_else(|| default_codex_home(dirs)))
}

fn default_codex_home(dirs: &BaseDirs) -> PathBuf {
    dirs.home_dir().join(".codex")
}

struct FileWrite {
    path: PathBuf,
    contents: Option<Vec<u8>>,
}

struct SelectPlan {
    config: PathBuf,
    catalog: PathBuf,
    desktop_providers: PathBuf,
    config_contents: Vec<u8>,
    catalog_contents: Option<Vec<u8>>,
    desktop_contents: Option<Vec<u8>>,
    restore_desktop_providers: bool,
    writes: Vec<FileWrite>,
}

fn select_plan(
    home: &Path,
    state: &State,
    previous_api_key: Option<&str>,
    desktop: bool,
    cli: bool,
) -> Result<SelectPlan> {
    let config = home.join("config.toml");
    let catalog = home.join("model-catalogs").join("auranion.json");
    let desktop_providers = home.join("desktop-model-providers.json");
    let auth = home.join("auth.json");
    let command = std::env::current_exe().context("locate Auranion executable for Codex auth")?;
    let legacy = legacy_config_document(&config, &desktop_providers, state)?;
    let mut document = legacy.unwrap_or(read_toml(&config)?);
    if !cli {
        restore_matching_cli_root_keys(&mut document, &config, state)?;
    }
    merge_document(&mut document, &catalog, &command, cli);
    let config_contents = document.to_string().into_bytes();
    let catalog_contents = serde_json::to_vec_pretty(&catalog_value())?;
    let preserve_catalog = match state.generated_for(&catalog) {
        Some(generated) if catalog.exists() => fs::read(&catalog)? != fs::read(generated)?,
        _ => false,
    };
    let catalog_contents = (!preserve_catalog).then_some(catalog_contents);
    let preserve_desktop_providers = match state.generated_for(&desktop_providers) {
        Some(generated) if desktop_providers.exists() => {
            fs::read(&desktop_providers)? != fs::read(generated)?
        }
        _ => false,
    };
    let desktop_contents = (desktop && !preserve_desktop_providers)
        .then(|| serde_json::to_vec_pretty(&desktop_providers_value()))
        .transpose()?;
    let restore_desktop_providers = !desktop && state.generated_for(&desktop_providers).is_some();
    let mut writes = vec![FileWrite {
        path: config.clone(),
        contents: Some(config_contents.clone()),
    }];
    if let Some(catalog_contents) = &catalog_contents {
        writes.push(FileWrite {
            path: catalog.clone(),
            contents: Some(catalog_contents.clone()),
        });
    }
    if let Some(desktop_contents) = &desktop_contents {
        writes.push(FileWrite {
            path: desktop_providers.clone(),
            contents: Some(desktop_contents.clone()),
        });
    } else if restore_desktop_providers
        && let Some(write) = restore_generated_file_write(&desktop_providers, state)?
    {
        writes.push(write);
    }
    if let Some(write) = legacy_auth_write(&auth, &desktop_providers, state, previous_api_key)? {
        writes.push(write);
    }
    Ok(SelectPlan {
        config,
        catalog,
        desktop_providers,
        config_contents,
        catalog_contents,
        desktop_contents,
        restore_desktop_providers,
        writes,
    })
}

fn restore_home_plan(
    home: &Path,
    state: &State,
    previous_api_key: Option<&str>,
) -> Result<Vec<FileWrite>> {
    let config = home.join("config.toml");
    let catalog = home.join("model-catalogs").join("auranion.json");
    let providers = home.join("desktop-model-providers.json");
    let auth = home.join("auth.json");
    let legacy = legacy_config_document(&config, &providers, state)?;
    let has_legacy = legacy.is_some();
    let current = legacy.unwrap_or(read_toml(&config)?);
    let restored = restore_toml_owned_document(current.clone(), &config, state)?;
    let mut writes = Vec::new();
    if let Some(document) = restored {
        writes.push(document_write(&config, document, state));
    } else if has_legacy {
        writes.push(FileWrite {
            path: config.clone(),
            contents: Some(current.to_string().into_bytes()),
        });
    }
    if let Some(write) = restore_generated_file_write(&providers, state)? {
        writes.push(write);
    }
    if let Some(write) = legacy_auth_write(&auth, &providers, state, previous_api_key)? {
        writes.push(write);
    }
    if let Some(write) = restore_catalog_write(&catalog, state)? {
        writes.push(write);
    }
    Ok(writes)
}

fn record_write_expected(state: &mut State, data_dir: &Path, write: &FileWrite) -> Result<()> {
    match &write.contents {
        Some(contents) => state.record_codex_expected(data_dir, &write.path, contents),
        None => state.record_codex_absent(&write.path),
    }
}

fn apply_transaction_writes(
    data_dir: &Path,
    state: &mut State,
    writes: Vec<FileWrite>,
) -> Result<()> {
    if let Err(error) = apply_writes(writes) {
        state.recover_codex_transaction()?;
        state.save(data_dir)?;
        state.complete_codex_transaction()?;
        state.save(data_dir)?;
        return Err(error);
    }
    state.finish_codex_transaction();
    Ok(())
}

fn apply_writes(writes: Vec<FileWrite>) -> Result<()> {
    for write in writes {
        match write.contents {
            Some(contents) => write_bytes(&write.path, &contents)?,
            None if write.path.exists() => {
                fs::remove_file(&write.path)
                    .with_context(|| format!("remove {}", write.path.display()))?;
            }
            None => {}
        }
    }
    Ok(())
}

fn document_write(path: &Path, document: DocumentMut, state: &State) -> FileWrite {
    if state.baseline_existed(path) == Some(false) && document.to_string().trim().is_empty() {
        FileWrite {
            path: path.to_path_buf(),
            contents: None,
        }
    } else {
        FileWrite {
            path: path.to_path_buf(),
            contents: Some(document.to_string().into_bytes()),
        }
    }
}

fn restore_file_write(path: &Path, state: &State) -> Result<Option<FileWrite>> {
    let Some(baseline) = state.baseline_for(path) else {
        return Ok(None);
    };
    Ok(Some(FileWrite {
        path: path.to_path_buf(),
        contents: state
            .baseline_existed(path)
            .unwrap_or(false)
            .then(|| fs::read(baseline))
            .transpose()?,
    }))
}

fn restore_generated_file_write(path: &Path, state: &State) -> Result<Option<FileWrite>> {
    let Some(generated) = state.generated_for(path) else {
        return Ok(None);
    };
    if path.exists() && fs::read(path)? == fs::read(generated)? {
        return restore_file_write(path, state);
    }
    Ok(None)
}

fn forget_restored_home_state(home: &Path, state: &mut State) {
    for path in [
        home.join("config.toml"),
        home.join("model-catalogs").join("auranion.json"),
        home.join("auth.json"),
        home.join("desktop-model-providers.json"),
    ] {
        state.forget_baseline(&path);
        state.forget_generated(&path);
    }
}

fn codex_home(dirs: &BaseDirs) -> Result<PathBuf> {
    let configured = std::env::var("CODEX_HOME")
        .ok()
        .filter(|path| !path.is_empty());
    codex_home_from_env(&default_codex_home(dirs), configured.as_deref())
}

fn codex_home_from_env(default: &Path, configured: Option<&str>) -> Result<PathBuf> {
    let Some(configured) = configured else {
        return Ok(default.to_path_buf());
    };
    let path = PathBuf::from(configured);
    let metadata = fs::metadata(&path).with_context(|| {
        format!("CODEX_HOME points to {configured:?}, but that path cannot be read")
    })?;
    if !metadata.is_dir() {
        bail!("CODEX_HOME points to {configured:?}, but that path is not a directory");
    }
    path.canonicalize()
        .with_context(|| format!("canonicalize CODEX_HOME {configured:?}"))
}

#[cfg(test)]
fn merge_config(path: &Path, catalog_path: &Path, command: &Path) -> Result<()> {
    let document = configured_document(path, catalog_path, command)?;
    write_catalog(catalog_path)?;
    write_toml(path, &document)
}

#[cfg(test)]
fn configured_document(path: &Path, catalog_path: &Path, command: &Path) -> Result<DocumentMut> {
    let mut document = read_toml(path)?;
    merge_document(&mut document, catalog_path, command, true);
    Ok(document)
}

fn merge_document(document: &mut DocumentMut, catalog_path: &Path, command: &Path, cli: bool) {
    if cli {
        document["model"] = DEFAULT_MODEL.into();
        document["model_provider"] = "auranion".into();
    }
    document["model_catalog_json"] = catalog_path.to_string_lossy().into_owned().into();

    let providers = ensure_table(&mut document["model_providers"]);
    let auranion = ensure_table(&mut providers["auranion"]);
    auranion["name"] = "Auranion".into();
    auranion["base_url"] = BASE_URL.into();
    auranion.remove("env_key");
    auranion.remove("env_key_instructions");
    auranion.remove("experimental_bearer_token");
    auranion["wire_api"] = "responses".into();
    auranion["requires_openai_auth"] = false.into();
    auranion["supports_websockets"] = false.into();

    let auth = ensure_table(&mut auranion["auth"]);
    let mut args = Array::new();
    args.push(TOKEN_COMMAND);
    auth["command"] = command.to_string_lossy().into_owned().into();
    auth["args"] = value(args);
    auth["timeout_ms"] = value(5_000);
    auth["refresh_interval_ms"] = value(0);
}

fn ensure_table(item: &mut Item) -> &mut Table {
    if item.is_inline_table() {
        *item = Item::Table(std::mem::take(item).into_table().expect("inline table"));
    } else if !item.is_table() {
        *item = Item::Table(Table::new());
    }
    item.as_table_mut().expect("table just created")
}

fn catalog_value() -> Value {
    let desktop = MODELS.iter().enumerate().map(|(priority, model)| {
        let efforts = supported_efforts(model.codex_desktop_reasoning_efforts);
        catalog_entry(
            model,
            model.codex_desktop_alias,
            codex_desktop_label(model.codex_desktop_alias),
            format!("{} via Auranion 9router", model.codex_desktop_alias),
            &efforts,
            "list",
            priority,
        )
    });
    let cli = MODELS.iter().enumerate().map(|(priority, model)| {
        let efforts = supported_efforts(model.reasoning_efforts);
        catalog_entry(
            model,
            model.upstream,
            model.label.to_owned(),
            format!("{} via Auranion", model.label),
            &efforts,
            "hide",
            priority + MODELS.len(),
        )
    });
    json!({ "models": desktop.chain(cli).collect::<Vec<_>>() })
}

/// The codex CLI parses the same `auranion.json` catalog for both the desktop
/// and CLI model lists, and its enum only accepts these levels. Catalog
/// entries must never emit `max`/`ultra`, or the whole file fails to parse
/// (`unknown variant 'max'`).
fn supported_efforts<'a>(efforts: &'a [&'a str]) -> Vec<&'a str> {
    efforts
        .iter()
        .copied()
        .filter(|effort| CODEX_REASONING_EFFORTS.contains(effort))
        .collect()
}

fn catalog_entry(
    model: &crate::catalog::Model,
    slug: &str,
    display_name: String,
    description: String,
    reasoning_efforts: &[&str],
    visibility: &str,
    priority: usize,
) -> Value {
    let mut entry = json!({
        "slug": slug,
        "display_name": display_name,
        "description": description,
        "supported_reasoning_levels": reasoning_efforts.iter().copied().map(reasoning_preset).collect::<Vec<_>>(),
        "shell_type": "shell_command",
        "visibility": visibility,
        "supported_in_api": true,
        "priority": priority,
        "additional_speed_tiers": [],
        "service_tiers": [],
        "availability_nux": null,
        "upgrade": null,
        "base_instructions": BASE_INSTRUCTIONS,
        "model_messages": {
            "instructions_template": BASE_INSTRUCTIONS,
            "instructions_variables": null
        },
        "supports_reasoning_summaries": true,
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
        "supports_search_tool": true
    });
    if reasoning_efforts.contains(&"medium") {
        entry["default_reasoning_level"] = "medium".into();
    }
    entry
}

fn codex_desktop_label(alias: &str) -> String {
    alias
        .split('-')
        .map(|part| {
            let mut chars = part.chars();
            match chars.next() {
                Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
                None => String::new(),
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

#[cfg(test)]
fn write_catalog(path: &Path) -> Result<()> {
    write_json(path, &catalog_value())
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
        "ultra" => "Maximum reasoning with automatic task delegation",
        _ => "Reasoning effort",
    };
    json!({ "effort": effort, "description": description })
}

fn legacy_config_document(
    config: &Path,
    providers: &Path,
    state: &State,
) -> Result<Option<DocumentMut>> {
    if state.baseline_for(providers).is_none() {
        return Ok(None);
    }
    let Some(baseline) = state.baseline_for(config) else {
        return Ok(None);
    };
    let current = read_toml(config)?;
    if !is_legacy_auranion_config(&current) {
        return Ok(None);
    }

    let original = read_toml(&baseline)?;
    let mut restored = current.clone();
    for key in LEGACY_ROOT_KEYS {
        if let Some(value) = original.get(key) {
            restored[key] = value.clone();
        } else {
            restored.remove(key);
        }
    }
    restore_table_keys(
        &mut restored,
        &original,
        "model_providers",
        "auranion",
        &OWNED_PROVIDER_KEYS,
    )?;
    if is_legacy_subagent(&current) {
        restore_table_keys(
            &mut restored,
            &original,
            "agents",
            "subagent",
            &LEGACY_SUBAGENT_KEYS,
        )?;
    }
    if is_legacy_profiles(&current) {
        restore_table(&mut restored, &original, "profiles");
    }
    Ok(Some(restored))
}

#[cfg(test)]
fn restore_legacy_config(config: &Path, providers: &Path, state: &State) -> Result<()> {
    let Some(document) = legacy_config_document(config, providers, state)? else {
        return Ok(());
    };
    write_toml(config, &document)?;
    if is_legacy_desktop_providers(providers)
        && let Some(write) = restore_file_write(providers, state)?
    {
        apply_writes(vec![write])?;
    }
    Ok(())
}

fn is_legacy_auranion_config(document: &DocumentMut) -> bool {
    document.get("model_catalog_json").is_some()
        && document.get("model").is_none()
        && document.get("model_provider").is_none()
        && document
            .get("model_providers")
            .and_then(Item::as_table)
            .and_then(|providers| providers.get("auranion"))
            .and_then(Item::as_table)
            .is_some_and(|provider| {
                provider.get("name").and_then(Item::as_str) == Some("Auranion")
                    && provider.get("base_url").and_then(Item::as_str) == Some(BASE_URL)
                    && provider.get("env_key").and_then(Item::as_str) == Some("OPENAI_API_KEY")
                    && provider.get("wire_api").and_then(Item::as_str) == Some("responses")
            })
}

fn is_legacy_subagent(document: &DocumentMut) -> bool {
    document
        .get("agents")
        .and_then(Item::as_table)
        .and_then(|agents| agents.get("subagent"))
        .and_then(Item::as_table)
        .is_some_and(|subagent| {
            subagent.get("description").and_then(Item::as_str) == Some("Auranion subagent")
                && subagent.get("model").and_then(Item::as_str) == Some(DEFAULT_MODEL)
        })
}

fn is_legacy_profiles(document: &DocumentMut) -> bool {
    let Some(profiles) = document.get("profiles").and_then(Item::as_table) else {
        return false;
    };

    profiles.iter().count() == MODELS.len()
        && MODELS.iter().all(|model| {
            profiles
                .get(model.label)
                .and_then(Item::as_table)
                .is_some_and(|profile| {
                    profile.iter().count() == 2
                        && profile.get("model").and_then(Item::as_str) == Some(model.upstream)
                        && profile.get("model_provider").and_then(Item::as_str) == Some("auranion")
                })
        })
}

fn desktop_providers_value() -> Value {
    let model_providers = MODELS
        .iter()
        .map(|model| {
            (
                model.codex_desktop_alias.to_string(),
                Value::String("auranion".into()),
            )
        })
        .collect::<Map<_, _>>();
    json!({
        "default_provider": "openai",
        "model_providers": model_providers
    })
}

#[cfg(test)]
fn legacy_desktop_providers() -> Value {
    let model_providers = MODELS
        .iter()
        .map(|model| (model.upstream.to_string(), Value::String("auranion".into())))
        .collect::<Map<_, _>>();
    json!({
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
    })
}

#[cfg(test)]
fn is_legacy_desktop_providers(path: &Path) -> bool {
    read_json(path).is_ok_and(|providers| providers == legacy_desktop_providers())
}

fn legacy_auth_write(
    path: &Path,
    providers: &Path,
    state: &State,
    previous_api_key: Option<&str>,
) -> Result<Option<FileWrite>> {
    if state.baseline_for(path).is_none()
        || !path.exists()
        || state.baseline_for(providers).is_none()
    {
        return Ok(None);
    }

    let saved_key = if previous_api_key.is_some() {
        None
    } else {
        super::super::keyring::load().ok().flatten()
    };
    let api_key = previous_api_key.or(saved_key.as_deref());
    let matches_legacy = read_json(path)
        .ok()
        .zip(api_key)
        .is_some_and(|(auth, api_key)| is_legacy_auranion_auth(&auth, api_key));
    if !matches_legacy {
        return Ok(None);
    }
    restore_file_write(path, state)
}

#[cfg(test)]
fn restore_legacy_auth(
    path: &Path,
    providers: &Path,
    state: &State,
    previous_api_key: Option<&str>,
) -> Result<()> {
    if let Some(write) = legacy_auth_write(path, providers, state, previous_api_key)? {
        apply_writes(vec![write])?;
    }
    Ok(())
}

fn is_legacy_auranion_auth(auth: &Value, api_key: &str) -> bool {
    let Some(auth) = auth.as_object() else {
        return false;
    };

    auth.len() == 2
        && auth.get("auth_mode").and_then(Value::as_str) == Some("apikey")
        && auth.get("OPENAI_API_KEY").and_then(Value::as_str) == Some(api_key)
}

fn restore_matching_cli_root_keys(
    current: &mut DocumentMut,
    path: &Path,
    state: &State,
) -> Result<()> {
    let (Some(baseline), Some(generated)) = (state.baseline_for(path), state.generated_for(path))
    else {
        return Ok(());
    };
    let original = read_toml(&baseline)?;
    let expected = read_toml(&generated)?;
    for key in CLI_ROOT_KEYS {
        if same_item(current.get(key), expected.get(key)) {
            restore_item(current, &original, key);
        }
    }
    Ok(())
}

fn restore_toml_owned_document(
    mut current: DocumentMut,
    path: &Path,
    state: &State,
) -> Result<Option<DocumentMut>> {
    let (Some(baseline), Some(generated)) = (state.baseline_for(path), state.generated_for(path))
    else {
        return Ok(None);
    };

    let original = read_toml(&baseline)?;
    let expected = read_toml(&generated)?;
    restore_matching_root_keys(&mut current, &original, &expected);
    restore_matching_provider_keys(&mut current, &original, &expected)?;
    Ok(Some(current))
}

#[cfg(test)]
fn restore_toml_owned(path: &Path, state: &State) -> Result<()> {
    let current = read_toml(path)?;
    let Some(document) = restore_toml_owned_document(current, path, state)? else {
        return Ok(());
    };
    apply_writes(vec![document_write(path, document, state)])
}

fn restore_matching_root_keys(
    current: &mut DocumentMut,
    original: &DocumentMut,
    expected: &DocumentMut,
) {
    for key in OWNED_ROOT_KEYS {
        if same_item(current.get(key), expected.get(key)) {
            restore_item(current, original, key);
        }
    }
}

fn same_item(current: Option<&Item>, expected: Option<&Item>) -> bool {
    match (current, expected) {
        (Some(current), Some(expected)) => {
            current
                .clone()
                .into_value()
                .ok()
                .map(|value| value.to_string())
                == expected
                    .clone()
                    .into_value()
                    .ok()
                    .map(|value| value.to_string())
        }
        (None, None) => true,
        _ => false,
    }
}

fn restore_item(current: &mut DocumentMut, original: &DocumentMut, key: &str) {
    if let Some(value) = original.get(key) {
        current[key] = value.clone();
    } else {
        current.remove(key);
    }
}

fn restore_matching_provider_keys(
    current: &mut DocumentMut,
    original: &DocumentMut,
    expected: &DocumentMut,
) -> Result<()> {
    if same_item(
        current.get("model_providers"),
        expected.get("model_providers"),
    ) {
        restore_item(current, original, "model_providers");
        return Ok(());
    }

    let original_provider = provider_item(original).cloned();
    let expected_provider = provider_item(expected).cloned();
    let provider_matches = same_item(provider_item(current), expected_provider.as_ref());
    if provider_matches {
        restore_provider_item(current, original_provider);
        remove_empty_provider(current);
        return Ok(());
    }

    let original_provider = original_provider.as_ref().and_then(Item::as_table_like);
    let expected_provider = expected_provider.as_ref().and_then(Item::as_table_like);
    let Some(current_provider) = provider_item_mut(current).and_then(Item::as_table_like_mut)
    else {
        return Ok(());
    };
    restore_matching_keys(
        current_provider,
        original_provider,
        expected_provider,
        &OWNED_PROVIDER_KEYS,
    );
    let current_auth = current_provider
        .get_mut("auth")
        .and_then(Item::as_table_like_mut);
    let original_auth = original_provider
        .and_then(|provider| provider.get("auth"))
        .and_then(Item::as_table_like);
    let expected_auth = expected_provider
        .and_then(|provider| provider.get("auth"))
        .and_then(Item::as_table_like);
    if let Some(current_auth) = current_auth {
        restore_matching_keys(current_auth, original_auth, expected_auth, &OWNED_AUTH_KEYS);
    }
    remove_empty_provider(current);
    Ok(())
}

fn provider_item(document: &DocumentMut) -> Option<&Item> {
    document
        .get("model_providers")
        .and_then(Item::as_table_like)
        .and_then(|providers| providers.get("auranion"))
}

fn provider_item_mut(document: &mut DocumentMut) -> Option<&mut Item> {
    document
        .get_mut("model_providers")
        .and_then(Item::as_table_like_mut)
        .and_then(|providers| providers.get_mut("auranion"))
}

fn restore_provider_item(current: &mut DocumentMut, original: Option<Item>) {
    let Some(providers) = current
        .get_mut("model_providers")
        .and_then(Item::as_table_like_mut)
    else {
        return;
    };
    match original {
        Some(provider) => {
            providers.insert("auranion", provider);
        }
        None => {
            providers.remove("auranion");
        }
    }
}

fn restore_matching_keys(
    current: &mut dyn TableLike,
    original: Option<&dyn TableLike>,
    expected: Option<&dyn TableLike>,
    keys: &[&str],
) {
    for key in keys {
        if same_item(current.get(key), expected.and_then(|table| table.get(key))) {
            match original.and_then(|table| table.get(key)) {
                Some(value) => {
                    current.insert(key, value.clone());
                }
                None => {
                    current.remove(key);
                }
            }
        }
    }
}

fn remove_empty_provider(document: &mut DocumentMut) {
    let remove_auranion = document
        .get("model_providers")
        .and_then(Item::as_table_like)
        .and_then(|providers| providers.get("auranion"))
        .and_then(Item::as_table_like)
        .is_some_and(TableLike::is_empty);
    if !remove_auranion {
        return;
    }

    let remove_providers = {
        let Some(providers) = document
            .get_mut("model_providers")
            .and_then(Item::as_table_like_mut)
        else {
            return;
        };
        providers.remove("auranion");
        providers.is_empty()
    };
    if remove_providers {
        document.remove("model_providers");
    }
}

fn restore_catalog_write(path: &Path, state: &State) -> Result<Option<FileWrite>> {
    let Some(generated) = state.generated_for(path) else {
        return Ok(None);
    };
    if path.exists() && fs::read(path)? == fs::read(generated)? {
        return restore_file_write(path, state);
    }
    Ok(None)
}

#[cfg(test)]
fn restore_catalog(path: &Path, state: &State) -> Result<()> {
    if let Some(write) = restore_catalog_write(path, state)? {
        apply_writes(vec![write])?;
    }
    Ok(())
}

fn restore_table(current: &mut DocumentMut, original: &DocumentMut, key: &str) {
    if let Some(value) = original.get(key) {
        current[key] = value.clone();
    } else {
        current.remove(key);
    }
}

fn restore_table_keys(
    current: &mut DocumentMut,
    original: &DocumentMut,
    parent: &str,
    child: &str,
    keys: &[&str],
) -> Result<()> {
    let Some(original_parent) = original.get(parent) else {
        return remove_generated_table_keys(current, parent, child, keys);
    };
    let Some(original_parent) = original_parent.as_table() else {
        current[parent] = original_parent.clone();
        return Ok(());
    };
    let Some(original_child) = original_parent.get(child) else {
        return remove_generated_table_keys(current, parent, child, keys);
    };
    let Some(original_child) = original_child.as_table() else {
        if !current.contains_key(parent) || !current[parent].is_table() {
            current[parent] = Item::Table(Table::new());
        }
        current[parent]
            .as_table_mut()
            .with_context(|| format!("{parent} is not a table"))?[child] = original_child.clone();
        return Ok(());
    };

    if !current.contains_key(parent) || !current[parent].is_table() {
        current[parent] = Item::Table(Table::new());
    }
    let current_parent = current[parent]
        .as_table_mut()
        .with_context(|| format!("{parent} is not a table"))?;
    if !current_parent.contains_key(child) || !current_parent[child].is_table() {
        current_parent[child] = Item::Table(Table::new());
    }
    let current_child = current_parent[child]
        .as_table_mut()
        .with_context(|| format!("{parent}.{child} is not a table"))?;

    for key in keys {
        if let Some(value) = original_child.get(key) {
            current_child[*key] = value.clone();
        } else {
            current_child.remove(key);
        }
    }
    Ok(())
}

fn remove_generated_table_keys(
    current: &mut DocumentMut,
    parent: &str,
    child: &str,
    keys: &[&str],
) -> Result<()> {
    let remove_parent = {
        let Some(current_parent) = current.get_mut(parent).and_then(Item::as_table_mut) else {
            return Ok(());
        };
        let Some(current_child) = current_parent.get_mut(child).and_then(Item::as_table_mut) else {
            return Ok(());
        };

        for key in keys {
            current_child.remove(key);
        }
        if current_child.iter().next().is_none() {
            current_parent.remove(child);
        }
        current_parent.iter().next().is_none()
    };

    if remove_parent {
        current.remove(parent);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_dir(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!("auranion-codex-{name}-{}", std::process::id()))
    }

    fn legacy_config() -> String {
        let mut config = format!(
            "model_catalog_json = \"C:/old/auranion.json\"\n\n[model_providers.auranion]\nname = \"Auranion\"\nbase_url = \"{BASE_URL}\"\nenv_key = \"OPENAI_API_KEY\"\nwire_api = \"responses\"\ncustom = \"keep\"\n\n[agents.subagent]\ndescription = \"Auranion subagent\"\nmodel = \"{DEFAULT_MODEL}\"\ncustom = \"keep\"\n"
        );
        for model in MODELS {
            config.push_str(&format!(
                "\n[profiles.\"{}\"]\nmodel = \"{}\"\nmodel_provider = \"auranion\"\n",
                model.label, model.upstream
            ));
        }
        config
    }

    fn merge_and_snapshot(
        state: &mut State,
        dir: &Path,
        config: &Path,
        catalog: &Path,
        command: &Path,
    ) {
        merge_config(config, catalog, command).unwrap();
        state.record_generated(dir, config).unwrap();
        state.record_generated(dir, catalog).unwrap();
    }

    fn persist_completed_transaction(state: &mut State, data_dir: &Path) {
        state.save(data_dir).unwrap();
        state.complete_codex_transaction().unwrap();
        state.save(data_dir).unwrap();
    }

    #[test]
    fn select_reselect_and_deselect_preserve_user_edits() {
        let dir = test_dir("public-lifecycle");
        let _ = std::fs::remove_dir_all(&dir);
        let home = dir.join(".codex");
        let config = home.join("config.toml");
        let catalog = home.join("model-catalogs").join("auranion.json");
        std::fs::create_dir_all(&home).unwrap();
        std::fs::write(
            &config,
            "model = \"gpt-5\"\nmodel_provider = \"openai\"\n\n[model_providers.openai]\nbase_url = \"https://api.openai.com/v1\"\n",
        )
        .unwrap();
        let mut state = State::default();

        select_at_home(&home, &home, &dir, &mut state, None).unwrap();
        persist_completed_transaction(&mut state, &dir);
        assert_eq!(state.codex_home, Some(home.clone()));
        assert_eq!(
            read_toml(&config)
                .unwrap()
                .get("model_provider")
                .and_then(Item::as_str),
            Some("auranion")
        );
        assert!(catalog.exists());

        let mut selected = read_toml(&config).unwrap();
        selected["model"] = "user-model".into();
        selected["model_providers"]["auranion"]["auth"]["command"] = "user-command".into();
        write_toml(&config, &selected).unwrap();

        select_at_home(&home, &home, &dir, &mut state, None).unwrap();
        persist_completed_transaction(&mut state, &dir);
        let reselected = read_toml(&config).unwrap();
        assert_eq!(
            reselected.get("model").and_then(Item::as_str),
            Some(DEFAULT_MODEL)
        );
        assert_eq!(
            reselected["model_providers"]["auranion"]["auth"]["command"].as_str(),
            Some(std::env::current_exe().unwrap().to_string_lossy().as_ref())
        );

        let mut edited = read_toml(&config).unwrap();
        edited["model"] = "user-model".into();
        edited["model_providers"]["auranion"]["auth"]["command"] = "user-command".into();
        write_toml(&config, &edited).unwrap();
        write_json(&catalog, &json!({"user_catalog": true})).unwrap();

        deselect_at_home(&home, &dir, &mut state, None).unwrap();
        persist_completed_transaction(&mut state, &dir);

        let restored = read_toml(&config).unwrap();
        assert_eq!(
            restored.get("model").and_then(Item::as_str),
            Some("user-model")
        );
        assert_eq!(
            restored.get("model_provider").and_then(Item::as_str),
            Some("openai")
        );
        assert_eq!(
            restored["model_providers"]["auranion"]["auth"]["command"].as_str(),
            Some("user-command")
        );
        assert_eq!(
            read_json(&catalog).unwrap().get("user_catalog"),
            Some(&json!(true))
        );
        assert_eq!(state.codex_home, None);
        assert!(state.baselines.is_empty());
        assert!(state.generated.is_empty());
        assert!(
            !dir.join("transactions")
                .read_dir()
                .unwrap()
                .next()
                .is_some()
        );

        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn interrupted_desktop_and_cli_apply_preserves_both_selections() {
        let dir = test_dir("desktop-cli-recovery");
        let _ = fs::remove_dir_all(&dir);
        let home = dir.join(".codex");
        let config = home.join("config.toml");
        let catalog = home.join("model-catalogs").join("auranion.json");
        let providers = home.join("desktop-model-providers.json");
        fs::create_dir_all(&home).unwrap();
        fs::write(&config, "model = \"gpt-5\"\n").unwrap();
        fs::write(&providers, "{\"user\":true}").unwrap();
        let mut state = State::default();

        reconcile_at_home(&home, &home, &dir, &mut state, true, true, None).unwrap();
        let mut edited = read_toml(&config).unwrap();
        edited["user_setting"] = "keep".into();
        write_toml(&config, &edited).unwrap();

        let mut recovered = State::load(&dir).unwrap();
        assert!(recovered.recover_codex_transaction().unwrap());
        assert!(recovered.active.contains(&Integration::CodexDesktop));
        assert!(recovered.active.contains(&Integration::CodexCli));
        assert_eq!(recovered.codex_home, Some(home));
        assert!(catalog.exists());
        assert_eq!(desktop_providers_value(), read_json(&providers).unwrap());
        persist_completed_transaction(&mut recovered, &dir);

        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn desktop_and_cli_share_catalog_without_clobbering_each_other() {
        let dir = test_dir("desktop-cli-shared-lifecycle");
        let _ = fs::remove_dir_all(&dir);
        let home = dir.join(".codex");
        let config = home.join("config.toml");
        let catalog = home.join("model-catalogs").join("auranion.json");
        let providers = home.join("desktop-model-providers.json");
        fs::create_dir_all(&home).unwrap();
        fs::write(&config, "model = \"gpt-5\"\nmodel_provider = \"openai\"\n").unwrap();
        fs::write(&providers, "{\"user\":true}").unwrap();
        let mut state = State::default();

        reconcile_at_home(&home, &home, &dir, &mut state, true, false, None).unwrap();
        persist_completed_transaction(&mut state, &dir);
        assert_eq!(
            read_toml(&config)
                .unwrap()
                .get("model_provider")
                .and_then(Item::as_str),
            Some("openai")
        );
        assert!(catalog.exists());
        assert_eq!(desktop_providers_value(), read_json(&providers).unwrap());

        reconcile_at_home(&home, &home, &dir, &mut state, true, true, None).unwrap();
        persist_completed_transaction(&mut state, &dir);
        assert_eq!(
            read_toml(&config)
                .unwrap()
                .get("model_provider")
                .and_then(Item::as_str),
            Some("auranion")
        );
        assert_eq!(desktop_providers_value(), read_json(&providers).unwrap());

        reconcile_at_home(&home, &home, &dir, &mut state, false, true, None).unwrap();
        persist_completed_transaction(&mut state, &dir);
        assert!(catalog.exists());
        assert_eq!(fs::read_to_string(&providers).unwrap(), "{\"user\":true}");

        deselect_at_home(&home, &dir, &mut state, None).unwrap();
        persist_completed_transaction(&mut state, &dir);
        assert_eq!(
            read_toml(&config)
                .unwrap()
                .get("model_provider")
                .and_then(Item::as_str),
            Some("openai")
        );
        assert!(!catalog.exists());
        assert_eq!(fs::read_to_string(&providers).unwrap(), "{\"user\":true}");

        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn reselect_preserves_modified_catalog_for_later_deselect() {
        let dir = test_dir("reselect-catalog-edit");
        let _ = fs::remove_dir_all(&dir);
        let home = dir.join(".codex");
        let config = home.join("config.toml");
        let catalog = home.join("model-catalogs").join("auranion.json");
        fs::create_dir_all(&home).unwrap();
        fs::write(&config, "model = \"gpt-5\"\n").unwrap();
        let mut state = State::default();

        select_at_home(&home, &home, &dir, &mut state, None).unwrap();
        persist_completed_transaction(&mut state, &dir);
        write_json(&catalog, &json!({"user_catalog": true})).unwrap();

        select_at_home(&home, &home, &dir, &mut state, None).unwrap();
        persist_completed_transaction(&mut state, &dir);
        assert_eq!(
            read_json(&catalog).unwrap().get("user_catalog"),
            Some(&json!(true))
        );

        deselect_at_home(&home, &dir, &mut state, None).unwrap();
        persist_completed_transaction(&mut state, &dir);
        assert_eq!(
            read_json(&catalog).unwrap().get("user_catalog"),
            Some(&json!(true))
        );

        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn interrupted_select_restores_original_files() {
        let dir = test_dir("interrupted-select");
        let _ = std::fs::remove_dir_all(&dir);
        let home = dir.join(".codex");
        let config = home.join("config.toml");
        let catalog = home.join("model-catalogs").join("auranion.json");
        let original = "model = \"gpt-5\"\nmodel_provider = \"openai\"\n";
        std::fs::create_dir_all(&home).unwrap();
        std::fs::write(&config, original).unwrap();
        let mut state = State::default();

        select_at_home(&home, &home, &dir, &mut state, None).unwrap();
        assert!(catalog.exists());

        let mut recovered = State::load(&dir).unwrap();
        assert!(recovered.recover_codex_transaction().unwrap());
        assert_eq!(std::fs::read_to_string(&config).unwrap(), original);
        assert!(!catalog.exists());
        assert_eq!(recovered.codex_home, None);
        assert!(recovered.baselines.is_empty());
        assert!(recovered.generated.is_empty());
        persist_completed_transaction(&mut recovered, &dir);
        let mut persisted = State::load(&dir).unwrap();
        assert!(!persisted.recover_codex_transaction().unwrap());

        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn select_failure_restores_config_written_before_catalog_error() {
        let dir = test_dir("partial-select");
        let _ = std::fs::remove_dir_all(&dir);
        let home = dir.join(".codex");
        let config = home.join("config.toml");
        let catalog_parent = home.join("model-catalogs");
        let original = "model = \"gpt-5\"\nmodel_provider = \"openai\"\n";
        std::fs::create_dir_all(&home).unwrap();
        std::fs::write(&config, original).unwrap();
        std::fs::write(&catalog_parent, "not a directory").unwrap();
        let mut state = State::default();

        assert!(select_at_home(&home, &home, &dir, &mut state, None).is_err());
        assert_eq!(std::fs::read_to_string(&config).unwrap(), original);
        assert_eq!(
            std::fs::read_to_string(&catalog_parent).unwrap(),
            "not a directory"
        );
        assert_eq!(state.codex_home, None);
        assert!(state.baselines.is_empty());
        assert!(state.generated.is_empty());

        let mut persisted = State::load(&dir).unwrap();
        assert!(!persisted.recover_codex_transaction().unwrap());

        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn failed_removal_rolls_back_earlier_writes() {
        let dir = test_dir("failed-removal");
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let config = dir.join("config.toml");
        let catalog = dir.join("auranion.json");
        fs::write(&config, "original config").unwrap();
        fs::write(&catalog, "generated catalog").unwrap();
        let mut state = State::default();
        state
            .begin_codex_transaction(&dir, &[&config, &catalog])
            .unwrap();
        state
            .record_codex_expected(&dir, &config, b"generated config")
            .unwrap();
        state.record_codex_absent(&catalog).unwrap();
        state.save(&dir).unwrap();
        fs::remove_file(&catalog).unwrap();
        fs::create_dir(&catalog).unwrap();

        assert!(
            apply_transaction_writes(
                &dir,
                &mut state,
                vec![
                    FileWrite {
                        path: config.clone(),
                        contents: Some(b"generated config".to_vec()),
                    },
                    FileWrite {
                        path: catalog.clone(),
                        contents: None,
                    },
                ],
            )
            .is_err()
        );
        assert_eq!(fs::read_to_string(&config).unwrap(), "original config");
        assert!(catalog.is_dir());

        let mut persisted = State::load(&dir).unwrap();
        assert!(!persisted.recover_codex_transaction().unwrap());

        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn deselect_without_generated_snapshots_preserves_unproven_files() {
        let dir = test_dir("legacy-baseline-only");
        let _ = fs::remove_dir_all(&dir);
        let home = dir.join(".codex");
        let config = home.join("config.toml");
        let catalog = home.join("model-catalogs").join("auranion.json");
        fs::create_dir_all(catalog.parent().unwrap()).unwrap();
        fs::write(
            &config,
            "model = \"cx/gpt-5.6-sol\"\nmodel_provider = \"auranion\"\n",
        )
        .unwrap();
        fs::write(&catalog, "{\"models\":[]}").unwrap();
        let config_before = fs::read(&config).unwrap();
        let catalog_before = fs::read(&catalog).unwrap();
        let mut state = State::default();
        state.codex_home = Some(home.clone());
        state.backup(&dir, &config).unwrap();
        state.backup(&dir, &catalog).unwrap();

        deselect_at_home(&home, &dir, &mut state, None).unwrap();
        persist_completed_transaction(&mut state, &dir);

        assert_eq!(fs::read(&config).unwrap(), config_before);
        assert_eq!(fs::read(&catalog).unwrap(), catalog_before);
        assert_eq!(state.codex_home, None);
        assert!(state.baselines.is_empty());
        assert!(state.generated.is_empty());

        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn completed_select_survives_recovery() {
        let dir = test_dir("completed-select");
        let _ = std::fs::remove_dir_all(&dir);
        let home = dir.join(".codex");
        let config = home.join("config.toml");
        let catalog = home.join("model-catalogs").join("auranion.json");
        std::fs::create_dir_all(&home).unwrap();
        std::fs::write(&config, "model = \"gpt-5\"\n").unwrap();
        let mut state = State::default();

        select_at_home(&home, &home, &dir, &mut state, None).unwrap();
        let selected_config = std::fs::read(&config).unwrap();
        let selected_catalog = std::fs::read(&catalog).unwrap();
        state.save(&dir).unwrap();

        let mut recovered = State::load(&dir).unwrap();
        assert!(recovered.recover_codex_transaction().unwrap());
        assert_eq!(std::fs::read(&config).unwrap(), selected_config);
        assert_eq!(std::fs::read(&catalog).unwrap(), selected_catalog);
        assert_eq!(recovered.codex_home, Some(home));
        persist_completed_transaction(&mut recovered, &dir);
        let mut persisted = State::load(&dir).unwrap();
        assert!(!persisted.recover_codex_transaction().unwrap());

        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn codex_home_uses_existing_override() {
        let dir = test_dir("home");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();

        assert_eq!(
            codex_home_from_env(Path::new("C:/default/.codex"), dir.to_str()).unwrap(),
            dir.canonicalize().unwrap()
        );

        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn codex_home_rejects_missing_override() {
        let dir = test_dir("missing-home");
        let _ = std::fs::remove_dir_all(&dir);

        assert!(codex_home_from_env(Path::new("C:/default/.codex"), dir.to_str()).is_err());
    }

    #[test]
    fn deselect_uses_saved_codex_home_when_environment_changes() {
        let dir = test_dir("saved-home");
        let _ = std::fs::remove_dir_all(&dir);
        let saved_home = dir.join("saved");
        let current_home = dir.join("current");
        std::fs::create_dir_all(&saved_home).unwrap();
        std::fs::create_dir_all(&current_home).unwrap();
        let config = saved_home.join("config.toml");
        let catalog = saved_home.join("model-catalogs").join("auranion.json");
        let command = dir.join("auranion");
        std::fs::write(&config, "model = \"gpt-5\"\n").unwrap();
        let mut state = State::default();
        state.codex_home = Some(saved_home.clone());
        state.backup(&dir, &config).unwrap();
        state.backup(&dir, &catalog).unwrap();
        merge_and_snapshot(&mut state, &dir, &config, &catalog, &command);

        let writes = restore_home_plan(&state.codex_home.clone().unwrap(), &state, None).unwrap();
        apply_writes(writes).unwrap();
        let restored = read_toml(&config).unwrap();
        assert_eq!(restored.get("model").and_then(Item::as_str), Some("gpt-5"));
        assert!(!catalog.exists());
        assert!(!current_home.join("config.toml").exists());

        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn merge_handles_non_table_model_providers_gracefully() {
        let dir = test_dir("nontable");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("config.toml");
        let catalog = dir.join("model-catalogs").join("auranion.json");
        let command = dir.join("auranion");
        std::fs::write(&path, "model_providers = \"invalid_string_value\"\n").unwrap();

        merge_config(&path, &catalog, &command).unwrap();
        let document = read_toml(&path).unwrap();
        assert!(document.get("model_providers").unwrap().is_table());

        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn merge_converts_inline_provider_tables_without_losing_custom_keys() {
        let dir = test_dir("inline-provider");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("config.toml");
        let catalog = dir.join("model-catalogs").join("auranion.json");
        let command = dir.join("auranion");
        std::fs::write(
            &path,
            "model_providers = { other = { base_url = \"https://other.example/v1\", custom = \"keep\" }, auranion = { custom = \"keep\", auth = { custom = \"keep\" } } }\n",
        )
        .unwrap();

        merge_config(&path, &catalog, &command).unwrap();
        let document = read_toml(&path).unwrap();
        let providers = document
            .get("model_providers")
            .and_then(Item::as_table_like)
            .unwrap();
        let other = providers
            .get("other")
            .and_then(Item::as_table_like)
            .unwrap();
        let auranion = providers
            .get("auranion")
            .and_then(Item::as_table_like)
            .unwrap();
        assert_eq!(
            other.get("base_url").and_then(Item::as_str),
            Some("https://other.example/v1")
        );
        assert_eq!(other.get("custom").and_then(Item::as_str), Some("keep"));
        assert_eq!(auranion.get("custom").and_then(Item::as_str), Some("keep"));
        assert_eq!(
            auranion
                .get("auth")
                .and_then(Item::as_table_like)
                .and_then(|auth| auth.get("custom"))
                .and_then(Item::as_str),
            Some("keep")
        );
        assert_eq!(
            auranion
                .get("auth")
                .and_then(Item::as_table_like)
                .and_then(|auth| auth.get("command"))
                .and_then(Item::as_str),
            command.to_string_lossy().as_ref().into()
        );

        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn merge_uses_isolated_provider_command_auth() {
        let dir = test_dir("merge");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("config.toml");
        let catalog = dir.join("model-catalogs").join("auranion.json");
        let command = dir.join("auranion helper");
        std::fs::write(
            &path,
            "preferred_auth_method = \"chatgpt\"\ncli_auth_credentials_store = \"auto\"\n\n[model_providers.auranion]\nenv_key = \"legacy\"\nenv_key_instructions = \"legacy\"\nexperimental_bearer_token = \"legacy\"\ncustom = \"keep\"\n\n[model_providers.auranion.auth]\ncommand = \"legacy\"\n\n[profiles.personal]\nmodel = \"gpt-5\"\n",
        )
        .unwrap();

        merge_config(&path, &catalog, &command).unwrap();
        let document = read_toml(&path).unwrap();
        let provider = &document["model_providers"]["auranion"];
        assert_eq!(
            document.get("model").and_then(Item::as_str),
            Some(DEFAULT_MODEL)
        );
        assert_eq!(
            document.get("model_provider").and_then(Item::as_str),
            Some("auranion")
        );
        assert_eq!(
            document.get("preferred_auth_method").and_then(Item::as_str),
            Some("chatgpt")
        );
        assert_eq!(
            document
                .get("cli_auth_credentials_store")
                .and_then(Item::as_str),
            Some("auto")
        );
        let provider_table = provider.as_table().unwrap();
        assert!(provider_table.get("env_key").is_none());
        assert!(provider_table.get("env_key_instructions").is_none());
        assert!(provider_table.get("experimental_bearer_token").is_none());
        assert_eq!(provider["requires_openai_auth"].as_bool(), Some(false));
        assert_eq!(provider["supports_websockets"].as_bool(), Some(false));
        assert_eq!(provider["custom"].as_str(), Some("keep"));
        assert_eq!(
            provider["auth"]["command"].as_str(),
            command.to_string_lossy().as_ref().into()
        );
        assert_eq!(
            provider["auth"]["args"]
                .as_array()
                .and_then(|args| args.get(0))
                .and_then(|arg| arg.as_str()),
            Some(TOKEN_COMMAND)
        );
        assert_eq!(provider["auth"]["timeout_ms"].as_integer(), Some(5_000));
        assert_eq!(
            provider["auth"]["refresh_interval_ms"].as_integer(),
            Some(0)
        );
        assert_eq!(
            document["profiles"]["personal"]["model"].as_str(),
            Some("gpt-5")
        );
        assert!(catalog.exists());

        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn merge_preserves_windows_paths_in_toml() {
        let dir = test_dir("windows-paths");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("config.toml");
        let catalog = dir.join("model-catalogs").join("auranion.json");
        let command = Path::new(r"C:\Program Files\Auranion\auranion.exe");

        merge_config(&path, &catalog, command).unwrap();
        let document = read_toml(&path).unwrap();
        assert_eq!(
            document.get("model_catalog_json").and_then(Item::as_str),
            catalog.to_string_lossy().as_ref().into()
        );
        assert_eq!(
            document["model_providers"]["auranion"]["auth"]["command"].as_str(),
            command.to_string_lossy().as_ref().into()
        );

        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn combined_catalog_preserves_desktop_and_cli_contracts() {
        let dir = test_dir("catalog");
        let _ = std::fs::remove_dir_all(&dir);
        let catalog = dir.join("auranion.json");
        write_catalog(&catalog).unwrap();

        let generated = read_json(&catalog).unwrap();
        let entries = generated.get("models").and_then(Value::as_array).unwrap();
        assert_eq!(entries.len(), MODELS.len() * 2);
        let slugs = entries
            .iter()
            .map(|entry| entry["slug"].as_str().unwrap())
            .collect::<std::collections::HashSet<_>>();
        assert_eq!(slugs.len(), MODELS.len() * 2);

        for model in MODELS {
            let desktop = entries
                .iter()
                .find(|entry| entry["slug"] == model.codex_desktop_alias)
                .expect("Desktop alias entry missing");
            let cli = entries
                .iter()
                .find(|entry| entry["slug"] == model.upstream)
                .expect("CLI entry missing");
            assert_eq!(desktop["visibility"], "list");
            assert_eq!(cli["visibility"], "hide");
            let desktop_efforts: Vec<_> = desktop["supported_reasoning_levels"]
                .as_array()
                .unwrap()
                .iter()
                .map(|effort| effort["effort"].as_str().unwrap())
                .collect();
            let expected_desktop = supported_efforts(model.codex_desktop_reasoning_efforts);
            assert_eq!(
                desktop_efforts,
                expected_desktop,
                "desktop catalog entry for {} must only emit levels the codex CLI accepts",
                model.codex_desktop_alias
            );
            let expected_cli = supported_efforts(model.reasoning_efforts);
            assert_eq!(
                cli["supported_reasoning_levels"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|effort| effort["effort"].as_str().unwrap())
                    .collect::<Vec<_>>(),
                expected_cli,
                "cli catalog entry for {} must only emit levels the codex CLI accepts",
                model.upstream
            );
            assert!(
                expected_desktop
                    .iter()
                    .chain(expected_cli.iter())
                    .all(|effort| matches!(
                        *effort,
                        "none" | "minimal" | "low" | "medium" | "high" | "xhigh"
                    ))
            );
        }
        for entry in entries {
            assert!(entry.get("model_messages").is_some());
            assert_eq!(
                entry.get("supports_reasoning_summaries"),
                Some(&json!(true))
            );
            assert_eq!(entry.get("availability_nux"), Some(&Value::Null));
            assert!(entry.get("supports_reasoning_summary_parameter").is_none());
            assert!(entry.get("use_responses_lite").is_none());
            assert!(entry.get("tool_mode").is_none());
            assert!(entry.get("multi_agent_version").is_none());
            assert!(entry.get("include_skills_usage_instructions").is_none());
        }

        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn desktop_provider_map_routes_aliases_only() {
        let providers = desktop_providers_value();
        assert_eq!(providers["default_provider"], "openai");
        let routes = providers["model_providers"].as_object().unwrap();
        assert_eq!(routes.len(), MODELS.len());
        for model in MODELS {
            assert_eq!(
                routes.get(model.codex_desktop_alias),
                Some(&Value::String("auranion".into()))
            );
            assert!(!routes.contains_key(model.upstream));
        }
    }

    #[test]
    fn deselect_restores_original_provider_fields() {
        let dir = test_dir("restore");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("config.toml");
        let catalog = dir.join("model-catalogs").join("auranion.json");
        let command = dir.join("auranion");
        std::fs::write(
            &path,
            "preferred_auth_method = \"chatgpt\"\ncli_auth_credentials_store = \"auto\"\nmodel = \"gpt-5\"\nmodel_provider = \"openai\"\n\n[model_providers.auranion]\nenv_key = \"legacy\"\ncustom = \"keep\"\n\n[model_providers.auranion.auth]\ncommand = \"legacy\"\n",
        )
        .unwrap();
        let mut state = State::default();
        state.backup(&dir, &path).unwrap();
        merge_and_snapshot(&mut state, &dir, &path, &catalog, &command);

        restore_toml_owned(&path, &state).unwrap();
        let restored = read_toml(&path).unwrap();
        assert_eq!(
            restored.get("preferred_auth_method").and_then(Item::as_str),
            Some("chatgpt")
        );
        assert_eq!(
            restored
                .get("cli_auth_credentials_store")
                .and_then(Item::as_str),
            Some("auto")
        );
        assert_eq!(restored.get("model").and_then(Item::as_str), Some("gpt-5"));
        assert_eq!(
            restored.get("model_provider").and_then(Item::as_str),
            Some("openai")
        );
        assert_eq!(
            restored["model_providers"]["auranion"]["env_key"].as_str(),
            Some("legacy")
        );
        assert_eq!(
            restored["model_providers"]["auranion"]["auth"]["command"].as_str(),
            Some("legacy")
        );
        assert_eq!(
            restored["model_providers"]["auranion"]["custom"].as_str(),
            Some("keep")
        );

        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn deselect_removes_generated_provider_table_when_baseline_lacks_it() {
        let dir = test_dir("remove-generated-provider");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("config.toml");
        let catalog = dir.join("model-catalogs").join("auranion.json");
        let command = dir.join("auranion");
        std::fs::write(&path, "[profiles.personal]\nmodel = \"gpt-5\"\n").unwrap();
        let mut state = State::default();
        state.backup(&dir, &path).unwrap();
        merge_and_snapshot(&mut state, &dir, &path, &catalog, &command);

        restore_toml_owned(&path, &state).unwrap();
        let restored = read_toml(&path).unwrap();
        assert!(restored.get("model_providers").is_none());
        assert_eq!(
            restored["profiles"]["personal"]["model"].as_str(),
            Some("gpt-5")
        );

        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn deselect_restores_scalar_model_providers_baseline() {
        let dir = test_dir("restore-scalar-provider");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("config.toml");
        let catalog = dir.join("model-catalogs").join("auranion.json");
        let command = dir.join("auranion");
        std::fs::write(&path, "model_providers = \"legacy\"\n").unwrap();
        let mut state = State::default();
        state.backup(&dir, &path).unwrap();
        merge_and_snapshot(&mut state, &dir, &path, &catalog, &command);

        restore_toml_owned(&path, &state).unwrap();
        let restored = read_toml(&path).unwrap();
        assert_eq!(
            restored.get("model_providers").and_then(Item::as_str),
            Some("legacy")
        );

        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn deselect_removes_empty_config_created_by_auranion() {
        let dir = test_dir("remove-created-config");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("config.toml");
        let catalog = dir.join("model-catalogs").join("auranion.json");
        let command = dir.join("auranion");
        let mut state = State::default();
        state.backup(&dir, &path).unwrap();
        merge_and_snapshot(&mut state, &dir, &path, &catalog, &command);

        restore_toml_owned(&path, &state).unwrap();
        assert!(!path.exists());

        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn deselect_preserves_user_edits_in_created_config() {
        let dir = test_dir("preserve-created-config-edit");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("config.toml");
        let catalog = dir.join("model-catalogs").join("auranion.json");
        let command = dir.join("auranion");
        let mut state = State::default();
        state.backup(&dir, &path).unwrap();
        merge_and_snapshot(&mut state, &dir, &path, &catalog, &command);
        let generated = std::fs::read_to_string(&path).unwrap();
        std::fs::write(
            &path,
            format!("{generated}\n[profiles.personal]\nmodel = \"gpt-5\"\n"),
        )
        .unwrap();

        restore_toml_owned(&path, &state).unwrap();
        let restored = read_toml(&path).unwrap();
        assert!(path.exists());
        assert!(restored.get("model").is_none());
        assert!(restored.get("model_provider").is_none());
        assert!(restored.get("model_catalog_json").is_none());
        assert!(restored.get("model_providers").is_none());
        assert_eq!(
            restored["profiles"]["personal"]["model"].as_str(),
            Some("gpt-5")
        );

        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn deselect_preserves_modified_owned_config_values() {
        let dir = test_dir("preserve-owned-edit");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("config.toml");
        let catalog = dir.join("model-catalogs").join("auranion.json");
        let command = dir.join("auranion");
        std::fs::write(&path, "model = \"gpt-5\"\n").unwrap();
        let mut state = State::default();
        state.backup(&dir, &path).unwrap();
        merge_and_snapshot(&mut state, &dir, &path, &catalog, &command);
        let mut generated = read_toml(&path).unwrap();
        generated["model"] = "user-model".into();
        generated["model_providers"]["auranion"]["auth"]["command"] = "user-command".into();
        write_toml(&path, &generated).unwrap();

        restore_toml_owned(&path, &state).unwrap();
        let restored = read_toml(&path).unwrap();
        assert_eq!(
            restored.get("model").and_then(Item::as_str),
            Some("user-model")
        );
        assert!(restored.get("model_provider").is_none());
        assert!(restored.get("model_catalog_json").is_none());
        let provider = restored
            .get("model_providers")
            .and_then(Item::as_table_like)
            .and_then(|providers| providers.get("auranion"))
            .and_then(Item::as_table_like);
        assert_eq!(
            provider
                .and_then(|provider| provider.get("auth"))
                .and_then(Item::as_table_like)
                .and_then(|auth| auth.get("command"))
                .and_then(Item::as_str),
            Some("user-command")
        );
        assert!(
            provider
                .and_then(|provider| provider.get("base_url"))
                .is_none()
        );

        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn deselect_preserves_modified_catalog() {
        let dir = test_dir("preserve-catalog-edit");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let config = dir.join("config.toml");
        let catalog = dir.join("model-catalogs").join("auranion.json");
        let command = dir.join("auranion");
        let mut state = State::default();
        state.backup(&dir, &config).unwrap();
        state.backup(&dir, &catalog).unwrap();
        merge_and_snapshot(&mut state, &dir, &config, &catalog, &command);
        let mut edited = read_json(&catalog).unwrap();
        edited["user"] = true.into();
        write_json(&catalog, &edited).unwrap();

        restore_catalog(&catalog, &state).unwrap();
        assert_eq!(read_json(&catalog).unwrap().get("user"), Some(&json!(true)));

        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn deselect_keeps_existing_empty_config() {
        let dir = test_dir("keep-empty-config");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("config.toml");
        let catalog = dir.join("model-catalogs").join("auranion.json");
        let command = dir.join("auranion");
        std::fs::write(&path, "").unwrap();
        let mut state = State::default();
        state.backup(&dir, &path).unwrap();
        merge_and_snapshot(&mut state, &dir, &path, &catalog, &command);

        restore_toml_owned(&path, &state).unwrap();
        assert!(path.exists());
        assert!(std::fs::read_to_string(&path).unwrap().trim().is_empty());

        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn legacy_upgrade_restores_generated_config_and_artifact() {
        let dir = test_dir("legacy-upgrade");
        let _ = std::fs::remove_dir_all(&dir);
        let codex_home = dir.join(".codex");
        std::fs::create_dir_all(&codex_home).unwrap();
        let config = codex_home.join("config.toml");
        let providers = codex_home.join("desktop-model-providers.json");
        let original = "preferred_auth_method = \"chatgpt\"\nmodel = \"gpt-5\"\nmodel_provider = \"openai\"\n\n[model_providers.auranion]\ncustom = \"original\"\n\n[agents.subagent]\ncustom = \"original\"\n\n[profiles.personal]\nmodel = \"gpt-5\"\n";
        std::fs::write(&config, original).unwrap();
        std::fs::write(&providers, "{\"user\":true}").unwrap();

        let mut state = State::default();
        state.backup(&dir, &config).unwrap();
        state.backup(&dir, &providers).unwrap();
        std::fs::write(&config, legacy_config()).unwrap();
        write_json(&providers, &legacy_desktop_providers()).unwrap();

        restore_legacy_config(&config, &providers, &state).unwrap();
        let restored = read_toml(&config).unwrap();
        assert_eq!(
            restored.get("preferred_auth_method").and_then(Item::as_str),
            Some("chatgpt")
        );
        assert_eq!(restored.get("model").and_then(Item::as_str), Some("gpt-5"));
        assert_eq!(
            restored.get("model_provider").and_then(Item::as_str),
            Some("openai")
        );
        assert_eq!(
            restored["model_providers"]["auranion"]["custom"].as_str(),
            Some("keep")
        );
        assert_eq!(
            restored["agents"]["subagent"]["custom"].as_str(),
            Some("keep")
        );
        assert_eq!(
            restored["profiles"]["personal"]["model"].as_str(),
            Some("gpt-5")
        );
        assert_eq!(
            std::fs::read_to_string(&providers).unwrap(),
            "{\"user\":true}"
        );

        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn legacy_upgrade_preserves_modified_artifacts() {
        let dir = test_dir("legacy-modified");
        let _ = std::fs::remove_dir_all(&dir);
        let codex_home = dir.join(".codex");
        std::fs::create_dir_all(&codex_home).unwrap();
        let config = codex_home.join("config.toml");
        let providers = codex_home.join("desktop-model-providers.json");
        std::fs::write(&config, "").unwrap();
        std::fs::write(&providers, "{\"user\":true}").unwrap();

        let mut state = State::default();
        state.backup(&dir, &config).unwrap();
        state.backup(&dir, &providers).unwrap();
        std::fs::write(
            &config,
            legacy_config().replace("Auranion subagent", "custom subagent"),
        )
        .unwrap();

        restore_legacy_config(&config, &providers, &state).unwrap();
        let restored = read_toml(&config).unwrap();
        assert!(restored.get("model_catalog_json").is_none());
        assert!(
            restored["model_providers"]["auranion"]
                .as_table()
                .unwrap()
                .get("env_key")
                .is_none()
        );
        assert_eq!(
            restored["agents"]["subagent"]["description"].as_str(),
            Some("custom subagent")
        );
        assert!(restored.get("profiles").is_none());
        assert_eq!(
            std::fs::read_to_string(&providers).unwrap(),
            "{\"user\":true}"
        );

        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn legacy_auth_restores_with_pre_rotation_key() {
        let dir = test_dir("legacy-auth");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let auth = dir.join("auth.json");
        let providers = dir.join("desktop-model-providers.json");
        std::fs::write(&auth, "{\"user\":true}").unwrap();
        std::fs::write(&providers, "{}").unwrap();
        let mut state = State::default();
        state.backup(&dir, &auth).unwrap();
        state.backup(&dir, &providers).unwrap();
        write_json(
            &auth,
            &json!({
                "auth_mode": "apikey",
                "OPENAI_API_KEY": "old-key"
            }),
        )
        .unwrap();

        restore_legacy_auth(&auth, &providers, &state, Some("old-key")).unwrap();
        assert_eq!(std::fs::read_to_string(&auth).unwrap(), "{\"user\":true}");

        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn legacy_auth_requires_matching_saved_key() {
        let auth = json!({
            "auth_mode": "apikey",
            "OPENAI_API_KEY": "auranion-test-key"
        });
        assert!(is_legacy_auranion_auth(&auth, "auranion-test-key"));
        assert!(!is_legacy_auranion_auth(&auth, "other-key"));
        assert!(!is_legacy_auranion_auth(
            &json!({
                "auth_mode": "apikey",
                "OPENAI_API_KEY": "auranion-test-key",
                "tokens": {}
            }),
            "auranion-test-key"
        ));
    }
}
