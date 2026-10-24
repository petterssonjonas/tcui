mod atomic;
mod migration;

use std::path::{Path, PathBuf};

use color_eyre::eyre::{eyre, Result, WrapErr};
use serde_json::Value;

use self::atomic::{
    acquire_lock, atomic_replace, maybe_fail, read_optional, restore_previous, write_backup_once,
    FailurePoint, CONFIG_FILE,
};
use self::migration::{deserialize_config, migrate_value, validate_catalog};
use super::AppConfig;

pub(crate) use atomic::atomic_replace_in;

#[cfg(test)]
pub(crate) use atomic::{inject_failure, ConfigError, TestFailurePoint};

const LEGACY_CONFIG_FILE: &str = "config.json";

pub(super) fn load() -> Result<AppConfig> {
    load_with_roots(dirs::config_dir(), dirs::data_dir())
}

fn load_with_roots(config_base: Option<PathBuf>, data_base: Option<PathBuf>) -> Result<AppConfig> {
    // Discover both roots before the first write. Never bootstrap into CWD.
    let config_root = required_root(config_base, "configuration")?;
    let data_root = required_root(data_base, "data")?;
    let config_path = config_root.join(CONFIG_FILE);
    let legacy_path = config_root.join(LEGACY_CONFIG_FILE);
    let config = {
        let _lock = acquire_lock(&config_root)?;
        if let Some(original) = read_optional(&config_path)? {
            load_toml_with_migration(&config_path, &original)?
        } else if let Some(original) = read_optional(&legacy_path)? {
            load_legacy_json(&original, &config_path)?
        } else {
            let config = AppConfig::default();
            atomic_replace(&config_path, toml::to_string_pretty(&config)?.as_bytes())?;
            reload_toml(&config_path)?
        }
    };
    bootstrap_layout(&config_root, &data_root)?;
    Ok(config)
}

pub(super) fn save(config: &AppConfig) -> Result<()> {
    save_with_root(config, dirs::config_dir())
}

fn save_with_root(config: &AppConfig, config_base: Option<PathBuf>) -> Result<()> {
    let config_root = required_root(config_base, "configuration")?;
    let _lock = acquire_lock(&config_root)?;
    validate_catalog(config)?;
    let content = toml::to_string_pretty(config)?;
    atomic_replace(&config_root.join(CONFIG_FILE), content.as_bytes())
}

fn load_toml_with_migration(path: &Path, original: &[u8]) -> Result<AppConfig> {
    let original_text = std::str::from_utf8(original).wrap_err("config.toml is not UTF-8")?;
    let original_toml: toml::Value = toml::from_str(original_text)?;
    let mut value = serde_json::to_value(&original_toml)?;
    migrate_value(&mut value)?;
    let config = deserialize_config(value)?;
    let rewritten = toml::to_string_pretty(&config)?;
    let rewritten_toml: toml::Value = toml::from_str(&rewritten)?;

    if original_toml == rewritten_toml {
        return Ok(config);
    }

    write_backup_once(path, original)?;
    atomic_replace(path, rewritten.as_bytes())?;
    match reload_toml(path) {
        Ok(reloaded) => Ok(reloaded),
        Err(reload_error) => {
            restore_previous(path, Some(original))?;
            Err(reload_error)
        }
    }
}

fn load_legacy_json(original: &[u8], config_path: &Path) -> Result<AppConfig> {
    let mut value: Value = serde_json::from_slice(original)?;
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
    let content = crate::protection::read_regular(path)?;
    let parsed: toml::Value = toml::from_str(std::str::from_utf8(&content)?)?;
    let mut value = serde_json::to_value(parsed)?;
    migrate_value(&mut value)?;
    deserialize_config(value)
}

fn bootstrap_layout(config_root: &Path, data_root: &Path) -> Result<()> {
    for dir in ["skills", "souls", "themes", "mcp"] {
        crate::protection::open_directory(&config_root.join(dir), true)?;
    }
    crate::protection::open_directory(data_root, true)?;
    crate::protection::open_directory(&data_root.join("vault"), true)?;
    Ok(())
}

fn required_root(base: Option<PathBuf>, kind: &str) -> Result<PathBuf> {
    let base = base.ok_or_else(|| eyre!("cannot determine {kind} directory"))?;
    if !base.is_absolute() {
        return Err(eyre!(
            "{kind} directory must be absolute: {}",
            base.display()
        ));
    }
    Ok(base.join("tcui"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_or_relative_roots_fail_before_config_creation() {
        let _guard = crate::test_support::env_lock().lock().unwrap();
        let root = std::env::temp_dir().join(format!(
            "tcui-missing-config-root-{:032x}",
            rand::random::<u128>()
        ));
        for (config, data) in [
            (None, Some(root.join("data"))),
            (Some(root.join("config")), None),
            (
                Some(PathBuf::from("relative-config")),
                Some(root.join("data")),
            ),
            (
                Some(root.join("config")),
                Some(PathBuf::from("relative-data")),
            ),
        ] {
            assert!(load_with_roots(config, data).is_err());
            assert!(!root.exists(), "root discovery must precede all writes");
        }
        let config = AppConfig::default();
        assert!(save_with_root(&config, None).is_err());
        assert!(save_with_root(&config, Some(PathBuf::from("relative-config"))).is_err());
        assert!(!root.exists());
    }
}
