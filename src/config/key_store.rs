use crate::config::AppConfig;
use color_eyre::Result;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::PathBuf;

pub struct KeyStore;

#[derive(Debug, Default, Clone, Serialize, Deserialize)]
struct StoredKeysFile {
    #[serde(default)]
    keys: BTreeMap<String, String>,
}

impl KeyStore {
    pub fn new() -> Self {
        Self
    }

    pub fn get(config: &AppConfig, provider: &str) -> Result<Option<String>> {
        let file = Self::load_file(config)?;
        file.keys
            .get(provider)
            .map(|encrypted| crate::storage::Storage::decrypt_shared_text(encrypted))
            .transpose()
    }

    pub fn save_keys(config: &AppConfig, keys: &[(String, String)]) -> Result<()> {
        let mut file = StoredKeysFile::default();
        for (provider, key) in keys {
            if key.trim().is_empty() {
                continue;
            }
            file.keys.insert(
                provider.clone(),
                crate::storage::Storage::encrypt_shared_text(key.trim())?,
            );
        }
        Self::write_file(config, &file)
    }

    fn load_file(config: &AppConfig) -> Result<StoredKeysFile> {
        let path = Self::keys_path(config)?;
        if !path.exists() {
            return Ok(StoredKeysFile::default());
        }
        let content = std::fs::read_to_string(path)?;
        Ok(toml::from_str(&content)?)
    }

    fn write_file(config: &AppConfig, file: &StoredKeysFile) -> Result<()> {
        let path = Self::keys_path(config)?;
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(&path, toml::to_string_pretty(file)?)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut perms = std::fs::metadata(&path)?.permissions();
            perms.set_mode(0o600);
            std::fs::set_permissions(&path, perms)?;
        }
        Ok(())
    }

    fn keys_path(config: &AppConfig) -> Result<PathBuf> {
        if let Some(path) = config
            .key_file
            .as_deref()
            .map(str::trim)
            .filter(|path| !path.is_empty())
        {
            return Ok(PathBuf::from(path));
        }

        let dir = dirs::data_dir().unwrap_or_else(|| PathBuf::from("."));
        Ok(dir.join("tcui").join("keys.toml"))
    }
}

impl Default for KeyStore {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    fn env_lock() -> &'static Mutex<()> {
        crate::test_support::env_lock()
    }

    fn unique_temp_dir(label: &str) -> PathBuf {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("system clock before epoch")
            .as_nanos();
        std::env::temp_dir().join(format!("tcui-{label}-{}-{nanos}", std::process::id()))
    }

    #[test]
    fn stores_keys_encrypted_with_shared_key() {
        let _guard = env_lock().lock().expect("env lock poisoned");
        let root = unique_temp_dir("keys-encryption");
        let config_home = root.join("config-home");
        std::fs::create_dir_all(&config_home).expect("create config dir");

        let mut config = AppConfig::default();
        config.key_file = Some(
            config_home
                .join("tcui")
                .join("keys.toml")
                .display()
                .to_string(),
        );

        KeyStore::save_keys(&config, &[("OpenAI".to_string(), "sk-secret".to_string())])
            .expect("save encrypted keys");
        let raw = std::fs::read_to_string(config_home.join("tcui").join("keys.toml"))
            .expect("read raw key file");
        assert!(raw.contains("enc:v1:"));
        assert!(!raw.contains("sk-secret"));

        let key = KeyStore::get(&config, "OpenAI").expect("load decrypted key");
        assert_eq!(key.as_deref(), Some("sk-secret"));

        std::fs::remove_dir_all(&root).expect("cleanup temp dir");
    }

    #[test]
    fn default_key_path_uses_xdg_data_home() {
        let _guard = env_lock().lock().expect("env lock poisoned");
        let root = unique_temp_dir("keys-default-path");
        let data_home = root.join("data-home");
        std::fs::create_dir_all(&data_home).expect("create data dir");
        std::env::set_var("XDG_DATA_HOME", &data_home);

        let config = AppConfig::default();
        KeyStore::save_keys(&config, &[("OpenAI".to_string(), "sk-secret".to_string())])
            .expect("save encrypted keys");

        let key_path = data_home.join("tcui").join("keys.toml");
        let raw = std::fs::read_to_string(&key_path).expect("read raw key file");
        assert!(raw.contains("enc:v1:"));
        assert!(!raw.contains("sk-secret"));

        std::fs::remove_dir_all(&root).expect("cleanup temp dir");
        std::env::remove_var("XDG_DATA_HOME");
    }
}
