use color_eyre::eyre::Result;
use serde_json::Value;

use super::atomic::ConfigError;
use crate::config::app_config::AppConfig;

pub(super) fn deserialize_config(value: Value) -> Result<AppConfig> {
    Ok(serde_json::from_value(value)?)
}

pub(super) fn migrate_value(value: &mut Value) -> Result<()> {
    let object = value
        .as_object_mut()
        .ok_or_else(|| ConfigError::InvalidConfig {
            message: "top-level value must be an object".to_string(),
        })?;
    // External selections are not import authorization. Keep only an explicit
    // legacy Saved directory for the separate, source-preserving migration.
    for key in [
        "vaults",
        "default_vault_id",
        "vault_path",
        "artifact_save_dir",
        "vault_operation_retention_days",
    ] {
        object.remove(key);
    }
    Ok(())
}
