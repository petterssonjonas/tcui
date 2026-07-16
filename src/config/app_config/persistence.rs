mod atomic;
mod migration;

use std::path::{Path, PathBuf};

use color_eyre::eyre::{Result, WrapErr};
use serde_json::Value;

use self::atomic::{
    acquire_lock, atomic_replace, maybe_fail, restore_previous, write_backup_once, FailurePoint,
    CONFIG_FILE,
};
use self::migration::{deserialize_config, migrate_value, validate_catalog};
use super::AppConfig;

#[cfg(test)]
pub(super) use atomic::{inject_failure, ConfigError, TestFailurePoint};

const LEGACY_CONFIG_FILE: &str = "config.json";

pub(super) fn load() -> Result<AppConfig> {
    let config_root = config_root();
    std::fs::create_dir_all(&config_root)?;
    let config_path = config_root.join(CONFIG_FILE);
    let legacy_path = config_root.join(LEGACY_CONFIG_FILE);
    let config = {
        let _lock = acquire_lock(&config_root)?;
        if config_path.exists() {
            load_toml_with_migration(&config_path)?
        } else if legacy_path.exists() {
            load_legacy_json(&legacy_path, &config_path)?
        } else {
            let config = AppConfig::default();
            atomic_replace(&config_path, toml::to_string_pretty(&config)?.as_bytes())?;
            reload_toml(&config_path)?
        }
    };
    bootstrap_layout(&config)?;
    Ok(config)
}

pub(super) fn save(config: &AppConfig) -> Result<()> {
    let config_root = config_root();
    std::fs::create_dir_all(&config_root)?;
    let _lock = acquire_lock(&config_root)?;
    validate_catalog(config)?;
    let content = toml::to_string_pretty(config)?;
    atomic_replace(&config_root.join(CONFIG_FILE), content.as_bytes())
}

fn load_toml_with_migration(path: &Path) -> Result<AppConfig> {
    let original = std::fs::read(path)?;
    let original_text = std::str::from_utf8(&original).wrap_err("config.toml is not UTF-8")?;
    let original_toml: toml::Value = toml::from_str(original_text)?;
    let mut value = serde_json::to_value(&original_toml)?;
    migrate_value(&mut value)?;
    let config = deserialize_config(value)?;
    let rewritten = toml::to_string_pretty(&config)?;
    let rewritten_toml: toml::Value = toml::from_str(&rewritten)?;

    if original_toml == rewritten_toml {
        return Ok(config);
    }

    write_backup_once(path, &original)?;
    atomic_replace(path, rewritten.as_bytes())?;
    match reload_toml(path) {
        Ok(reloaded) => Ok(reloaded),
        Err(reload_error) => {
            restore_previous(path, Some(&original))?;
            Err(reload_error)
        }
    }
}

fn load_legacy_json(legacy_path: &Path, config_path: &Path) -> Result<AppConfig> {
    let original = std::fs::read(legacy_path)?;
    let mut value: Value = serde_json::from_slice(&original)?;
    migrate_value(&mut value)?;
    let config = deserialize_config(value)?;
    let rendered = toml::to_string_pretty(&config)?;
    atomic_replace(config_path, rendered.as_bytes())?;
    match reload_toml(config_path) {
        Ok(reloaded) => Ok(reloaded),
        Err(reload_error) => {
            restore_previous(config_path, None)?;
            Err(reload_error)
        }
    }
}

fn reload_toml(path: &Path) -> Result<AppConfig> {
    maybe_fail(FailurePoint::Reload)?;
    let content = std::fs::read_to_string(path)?;
    let parsed: toml::Value = toml::from_str(&content)?;
    let mut value = serde_json::to_value(parsed)?;
    migrate_value(&mut value)?;
    deserialize_config(value)
}

fn bootstrap_layout(config: &AppConfig) -> Result<()> {
    let config_root = config_root();
    for dir in ["skills", "souls", "themes", "mcp"] {
        std::fs::create_dir_all(config_root.join(dir))?;
    }
    std::fs::create_dir_all(data_root())?;
    crate::config::key_store::KeyStore::save_keys(config, &[])?;
    Ok(())
}

fn config_root() -> PathBuf {
    dirs::config_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("tcui")
}

fn data_root() -> PathBuf {
    dirs::data_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("tcui")
}
