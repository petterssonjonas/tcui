use std::path::PathBuf;
use std::str::FromStr;

use color_eyre::eyre::Result;
use serde_json::{Map, Value};

use super::atomic::ConfigError;
use crate::config::app_config::AppConfig;
use crate::config::vault_profiles::{normalize_name, VaultCatalog, VaultProfile, VaultProfileId};

pub(super) fn deserialize_config(value: Value) -> Result<AppConfig> {
    let mut config: AppConfig = serde_json::from_value(value)?;
    validate_catalog(&config)?;
    config.vault_path = config
        .default_vault_id
        .as_ref()
        .and_then(|id| config.vaults.get(id))
        .map(|profile| profile.root.display().to_string());
    config.artifact_save_dir = config.saved_files_dir.clone();
    Ok(config)
}

pub(super) fn validate_catalog(config: &AppConfig) -> Result<()> {
    let catalog = VaultCatalog {
        profiles: config.vaults.clone(),
        default_id: config.default_vault_id.clone(),
    };
    catalog.validate()?;
    Ok(())
}

pub(super) fn migrate_value(value: &mut Value) -> Result<()> {
    let object = value
        .as_object_mut()
        .ok_or_else(|| ConfigError::InvalidConfig {
            message: "top-level value must be an object".to_string(),
        })?;
    let legacy_vault = object.remove("vault_path");
    let legacy_artifacts = object.remove("artifact_save_dir");
    let has_new_catalog = match object.get("vaults") {
        Some(Value::Object(vaults)) => !vaults.is_empty(),
        Some(Value::Null) | None => false,
        Some(_) => true,
    };

    if !has_new_catalog {
        migrate_legacy_vault(object, legacy_vault)?;
    }
    install_internal_profile(object)?;
    if !object.contains_key("saved_files_dir") {
        if let Some(saved_files_dir) = legacy_artifacts {
            object.insert("saved_files_dir".to_string(), saved_files_dir);
        }
    }
    Ok(())
}

fn migrate_legacy_vault(
    object: &mut Map<String, Value>,
    legacy_vault: Option<Value>,
) -> Result<()> {
    let Some(legacy_vault) = legacy_vault else {
        return Ok(());
    };
    let path = legacy_vault
        .as_str()
        .ok_or_else(|| ConfigError::InvalidConfig {
            message: "vault_path must be a string".to_string(),
        })?
        .trim();
    if path.is_empty() {
        return Ok(());
    }

    let hash = blake3::hash(path.as_bytes()).to_hex().to_string();
    let id = VaultProfileId::from_str(&hash[..32])?;
    let profile = VaultProfile::from_persisted(id.clone(), "Default", expand_tilde(path))?;
    let mut vaults = Map::new();
    vaults.insert(id.to_string(), serde_json::to_value(profile)?);
    object.insert("vaults".to_string(), Value::Object(vaults));
    object.insert(
        "default_vault_id".to_string(),
        Value::String(id.to_string()),
    );
    Ok(())
}

fn expand_tilde(path: &str) -> PathBuf {
    if path == "~" {
        dirs::home_dir().unwrap_or_else(|| PathBuf::from(path))
    } else if let Some(rest) = path.strip_prefix("~/") {
        dirs::home_dir()
            .map(|home| home.join(rest))
            .unwrap_or_else(|| PathBuf::from(path))
    } else {
        PathBuf::from(path)
    }
}

fn install_internal_profile(object: &mut Map<String, Value>) -> Result<()> {
    let mut profile = VaultProfile::internal();
    let mut id = profile.id.to_string();
    let root = serde_json::to_value(&profile.root)?;
    let vaults = object
        .entry("vaults")
        .or_insert_with(|| Value::Object(Map::new()));
    if vaults.is_null() {
        *vaults = Value::Object(Map::new());
    }
    if let Some(vaults) = vaults.as_object_mut() {
        if let Some((existing_id, _)) = vaults
            .iter()
            .find(|(_, value)| value.get("root") == Some(&root))
        {
            id = existing_id.clone();
        } else {
            let mut suffix = 2;
            while vaults
                .values()
                .filter_map(|value| value.get("display_name").and_then(Value::as_str))
                .any(|name| normalize_name(name) == profile.name_key)
            {
                profile.display_name = format!("TCUI Internal {suffix}");
                profile.name_key = normalize_name(&profile.display_name);
                suffix += 1;
            }
            vaults
                .entry(id.clone())
                .or_insert(serde_json::to_value(profile)?);
        }
    }
    if object.get("default_vault_id").is_none_or(Value::is_null) {
        object.insert("default_vault_id".to_string(), Value::String(id));
    }
    Ok(())
}
