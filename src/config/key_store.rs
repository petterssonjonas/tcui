use crate::config::AppConfig;
use crate::storage::crypto::{
    SharedKey, StorageCryptoError, decrypt_serialized, encrypt_serialized,
};
use crate::storage::paths::TcuiDataPaths;
use color_eyre::Result;
use std::path::PathBuf;
use std::sync::{Mutex, MutexGuard, OnceLock};

#[cfg_attr(
    not(test),
    expect(
        dead_code,
        reason = "Todo 7 invokes the typed API-key lifecycle from the OpenRouter auth command."
    )
)]
mod api_key;
#[cfg_attr(
    not(test),
    expect(
        dead_code,
        reason = "Todo 7 invokes the typed API-key lifecycle from the OpenRouter auth command."
    )
)]
mod credential;
mod format;
mod oauth;
mod path_security;
mod persistence;
mod rollback;

pub use credential::{
    ApiKeyCredential, ApiKeyCredentialOwnership, ApiKeyCredentialSource, Credential,
};
use format::{LoadedKeysFile, StoredKeysFile};
use oauth::StoredOAuthCredential;
pub use oauth::{KeyStoreError, OAuthCredential};
pub use oauth::{OAuthCredentialOwnership, OAuthCredentialSource};

const CURRENT_STORE_VERSION: u32 = 1;

pub struct KeyStore;

impl KeyStore {
    pub fn new() -> Self {
        Self
    }

    pub fn get(config: &AppConfig, provider: &str) -> Result<Option<String>> {
        let file = Self::load_file(config)?;
        file.keys()
            .get(provider)
            .map(|encrypted| crate::storage::Storage::decrypt_shared_text(encrypted))
            .transpose()
    }

    pub fn save_keys(config: &AppConfig, keys: &[(String, String)]) -> Result<()> {
        let _guard = store_write_guard()?;
        let mut file = Self::load_file(config)?.into_current()?;
        file.keys.clear();
        for (provider, key) in keys {
            if key.trim().is_empty() {
                continue;
            }
            file.keys.insert(
                provider.clone(),
                crate::storage::Storage::encrypt_shared_text(key.trim())?,
            );
        }
        Self::write_file(config, &file)?;
        Ok(())
    }

    pub fn get_oauth(
        config: &AppConfig,
        provider: &str,
    ) -> std::result::Result<Option<OAuthCredential>, KeyStoreError> {
        #[cfg(not(unix))]
        {
            let _ = (config, provider);
            return Err(KeyStoreError::UnsupportedPlatform);
        }

        let file = Self::load_file(config)?.into_current()?;
        let Some(encrypted) = file.oauth.get(provider) else {
            return Ok(None);
        };
        let shared_key = shared_key()?;
        let payload = decrypt_serialized::<StoredOAuthCredential>(
            &shared_key,
            &oauth_record_kind(provider),
            encrypted,
        )
        .map_err(map_oauth_crypto_error)?;
        let credential = OAuthCredential::from_payload(payload)?;
        if credential.provider != provider {
            return Err(KeyStoreError::ProviderMismatch);
        }
        Ok(Some(credential))
    }

    pub fn upsert_oauth(
        config: &AppConfig,
        credential: &OAuthCredential,
    ) -> std::result::Result<(), KeyStoreError> {
        #[cfg(not(unix))]
        {
            let _ = (config, credential);
            return Err(KeyStoreError::UnsupportedPlatform);
        }

        let _guard = store_write_guard()?;
        credential.validate()?;
        let mut file = Self::load_file(config)?.into_current()?;
        let shared_key = shared_key()?;
        let encrypted = encrypt_serialized(
            &shared_key,
            &oauth_record_kind(&credential.provider),
            &StoredOAuthCredential::from(credential),
        )
        .map_err(map_oauth_crypto_error)?;
        file.oauth.insert(credential.provider.clone(), encrypted);
        Self::write_file(config, &file)
    }

    pub fn remove_oauth(
        config: &AppConfig,
        provider: &str,
    ) -> std::result::Result<bool, KeyStoreError> {
        #[cfg(not(unix))]
        {
            let _ = (config, provider);
            return Err(KeyStoreError::UnsupportedPlatform);
        }

        let _guard = store_write_guard()?;
        let mut file = Self::load_file(config)?.into_current()?;
        let removed = file.oauth.remove(provider).is_some();
        if removed {
            Self::write_file(config, &file)?;
        }
        Ok(removed)
    }

    fn load_file(config: &AppConfig) -> std::result::Result<LoadedKeysFile, KeyStoreError> {
        let path = Self::keys_path(config).map_err(|_| KeyStoreError::KeyPath)?;
        persistence::read_file(&path)
    }

    fn write_file(
        config: &AppConfig,
        file: &StoredKeysFile,
    ) -> std::result::Result<(), KeyStoreError> {
        let path = Self::keys_path(config).map_err(|_| KeyStoreError::KeyPath)?;
        persistence::write_file(&path, file)
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

fn shared_key() -> std::result::Result<SharedKey, KeyStoreError> {
    SharedKey::load_or_create_default(&TcuiDataPaths::discover())
        .map(|loaded| loaded.key)
        .map_err(|_| KeyStoreError::KeyAccess)
}

fn store_write_guard() -> std::result::Result<MutexGuard<'static, ()>, KeyStoreError> {
    static STORE_WRITE_LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    STORE_WRITE_LOCK
        .get_or_init(|| Mutex::new(()))
        .lock()
        .map_err(|_| KeyStoreError::Write)
}

fn oauth_record_kind(provider: &str) -> String {
    format!("oauth:{provider}")
}

fn map_oauth_crypto_error(error: StorageCryptoError) -> KeyStoreError {
    match error {
        StorageCryptoError::Encrypt => KeyStoreError::OauthEncrypt,
        StorageCryptoError::Json(_) | StorageCryptoError::Utf8(_) => {
            KeyStoreError::InvalidOauthPayload
        }
        StorageCryptoError::Io(_)
        | StorageCryptoError::InvalidKeyLength
        | StorageCryptoError::MissingDefaultKey { .. } => KeyStoreError::KeyAccess,
        StorageCryptoError::Base64(_)
        | StorageCryptoError::MissingNonce
        | StorageCryptoError::MissingCiphertext
        | StorageCryptoError::InvalidEnvelope
        | StorageCryptoError::Decrypt
        | StorageCryptoError::WrongDocumentKind => KeyStoreError::OauthDecrypt,
    }
}

impl Default for KeyStore {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod test_support;

#[cfg(test)]
mod legacy_tests;

#[cfg(test)]
mod oauth_api_tests;

#[cfg(test)]
mod credential_tests;

#[cfg(test)]
mod oauth_persistence_tests;

#[cfg(test)]
mod oauth_concurrency_tests;

#[cfg(test)]
mod filesystem_tests;

#[cfg(test)]
mod parent_security_tests;

#[cfg(test)]
mod future_version_tests;
