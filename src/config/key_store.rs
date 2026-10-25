use crate::config::AppConfig;
use color_eyre::Result;
use std::path::PathBuf;
use zeroize::Zeroizing;

mod api_key;
mod credential;
mod format;
mod oauth;
mod path_security;
mod persistence;
mod vault;

use credential::StoredApiKeyCredential;
pub use credential::{
    ApiKeyCredential, ApiKeyCredentialOwnership, ApiKeyCredentialSource, Credential,
};
use oauth::StoredOAuthCredential;
pub use oauth::{KeyStoreError, OAuthCredential, OAuthCredentialOwnership, OAuthCredentialSource};
use vault::Vault;
pub use vault::{CredentialStorage, SecretEntry, SecretKind, VaultStatus};
pub(crate) use vault::{PasswordChange, ProtectionWrapping};

const CURRENT_STORE_VERSION: u32 = 1;

pub struct KeyStore;

/// Connection identity stays separate from its provider family and display label.
pub(crate) fn provider_base_name(provider: &str) -> &str {
    match provider.rsplit_once("::") {
        Some((base, suffix))
            if matches!(suffix.split(':').next(), Some("encrypted" | "plaintext")) =>
        {
            base
        }
        _ => provider,
    }
}

impl KeyStore {
    pub fn new() -> Self {
        Self
    }

    pub fn has_credential(
        _config: &AppConfig,
        provider: &str,
    ) -> std::result::Result<bool, KeyStoreError> {
        Ok(Self::provider_storage(_config, provider)?.is_some())
    }

    /// Returns storage metadata without decrypting or requiring an unlocked vault.
    pub fn provider_storage(
        _config: &AppConfig,
        provider: &str,
    ) -> std::result::Result<Option<CredentialStorage>, KeyStoreError> {
        Vault::open()?.provider_storage(provider)
    }

    /// Finds a separate identity when changing storage, never replacing another connection.
    pub(crate) fn connection_id(
        _config: &AppConfig,
        provider: &str,
        storage: CredentialStorage,
    ) -> std::result::Result<String, KeyStoreError> {
        let store = Vault::open()?;
        if store
            .provider_storage(provider)?
            .is_none_or(|existing| existing == storage)
        {
            return Ok(provider.to_owned());
        }
        let base = provider_base_name(provider);
        let suffix = if storage == CredentialStorage::Encrypted {
            "encrypted"
        } else {
            "plaintext"
        };
        let mut candidate = format!("{base}::{suffix}");
        let mut number = 2_u64;
        while store
            .provider_storage(&candidate)?
            .is_some_and(|existing| existing != storage)
        {
            candidate = format!("{base}::{suffix}:{number}");
            number = number
                .checked_add(1)
                .ok_or(KeyStoreError::InvalidCredentialPayload)?;
        }
        Ok(candidate)
    }

    /// Changes the default for new records only; existing credentials are untouched.
    pub fn set_storage_default(
        _config: &AppConfig,
        storage: CredentialStorage,
    ) -> std::result::Result<(), KeyStoreError> {
        Vault::open()?.set_default(storage)
    }

    pub(crate) fn protection_key(
        create: bool,
    ) -> std::result::Result<Zeroizing<[u8; 32]>, KeyStoreError> {
        let keys = vault::session()?;
        let store = Vault::open()?;
        store.require_key(&keys)?;
        store.transaction(|| {
            store.authenticate_unlocked(&keys)?;
            if let Some(payload) = store.read("app_data_key", "master", &keys)? {
                let bytes: [u8; 32] = payload
                    .as_slice()
                    .try_into()
                    .map_err(|_| KeyStoreError::InvalidCredentialPayload)?;
                return Ok(Zeroizing::new(bytes));
            }
            if !create {
                return Err(KeyStoreError::InvalidCredentialPayload);
            }
            let key = Zeroizing::new(rand::random::<[u8; 32]>());
            store.write(
                "app_data_key",
                "master",
                "Application data key",
                key.as_ref(),
                CredentialStorage::Encrypted,
                &keys,
            )?;
            Ok(key)
        })
    }

    /// Exports only the authenticated password wrapping for the application data master.
    /// This never creates a key and never exports provider credentials.
    pub(crate) fn protection_wrapping() -> std::result::Result<ProtectionWrapping, KeyStoreError> {
        let keys = vault::session()?;
        Vault::open()?.protection_wrapping(&keys)
    }

    pub(crate) fn prepare_password_change(
        new_password: &str,
    ) -> std::result::Result<PasswordChange, KeyStoreError> {
        let keys = vault::session()?;
        Vault::open()?.prepare_password_change(new_password, &keys)
    }

    pub(crate) fn apply_password_change(
        change: &PasswordChange,
    ) -> std::result::Result<(), KeyStoreError> {
        let mut keys = vault::session()?;
        Vault::open()?.apply_password_change(change, &mut keys, true)
    }

    pub(crate) fn validate_password_change(
        change: &PasswordChange,
    ) -> std::result::Result<(), KeyStoreError> {
        let mut keys = vault::session()?;
        Vault::open()?.apply_password_change(change, &mut keys, false)
    }

    pub fn vault_status(config: &AppConfig) -> std::result::Result<VaultStatus, KeyStoreError> {
        let keys = vault::session()?;
        let store = Vault::open()?;
        Ok(VaultStatus {
            initialized: store.initialized()?,
            unlocked: store.unlocked(&keys),
            storage: store.preference()?,
            legacy_present: Self::keys_path(config)?.exists(),
        })
    }

    pub fn initialize_vault(
        _config: &AppConfig,
        password: &str,
    ) -> std::result::Result<(), KeyStoreError> {
        let mut keys = vault::session()?;
        Vault::open()?.initialize(password, &mut keys)
    }

    pub fn unlock_vault(
        _config: &AppConfig,
        password: &str,
    ) -> std::result::Result<(), KeyStoreError> {
        let mut keys = vault::session()?;
        keys.remove(&crate::storage::paths::TcuiDataPaths::discover().database);
        Vault::open()?.unlock(password, &mut keys)
    }

    pub fn lock_vault(_config: &AppConfig) -> std::result::Result<(), KeyStoreError> {
        let mut keys = vault::session()?;
        keys.remove(&crate::storage::paths::TcuiDataPaths::discover().database);
        Ok(())
    }

    pub fn set_storage_preference(
        _config: &AppConfig,
        storage: CredentialStorage,
    ) -> std::result::Result<(), KeyStoreError> {
        let keys = vault::session()?;
        Vault::open()?.set_preference(storage, &keys)
    }

    pub fn get(_config: &AppConfig, provider: &str) -> Result<Option<String>> {
        let keys = vault::session()?;
        let store = Vault::open()?;
        let Some(payload) = store.read("api_key", provider, &keys)? else {
            return Ok(None);
        };
        Ok(Some(
            std::str::from_utf8(&payload)
                .map_err(|_| KeyStoreError::InvalidCredentialPayload)?
                .to_owned(),
        ))
    }

    #[cfg(test)]
    pub fn set(
        _config: &AppConfig,
        provider: &str,
        key: &str,
    ) -> std::result::Result<(), KeyStoreError> {
        let keys = vault::session()?;
        let store = Vault::open()?;
        if key.trim().is_empty() {
            return Err(KeyStoreError::InvalidCredentialPayload);
        }
        store.transaction(|| {
            store.write(
                "api_key",
                provider,
                provider,
                key.trim().as_bytes(),
                store.selected_storage(provider, None)?,
                &keys,
            )
        })
    }

    pub fn set_with_storage(
        _config: &AppConfig,
        provider: &str,
        key: &str,
        storage: CredentialStorage,
    ) -> std::result::Result<(), KeyStoreError> {
        if key.trim().is_empty() {
            return Err(KeyStoreError::InvalidCredentialPayload);
        }
        let keys = vault::session()?;
        let store = Vault::open()?;
        store.transaction(|| {
            if store
                .provider_storage(provider)?
                .is_some_and(|existing| existing != storage)
            {
                return Err(KeyStoreError::InvalidCredentialPayload);
            }
            store.write(
                "api_key",
                provider,
                provider,
                key.trim().as_bytes(),
                storage,
                &keys,
            )
        })
    }

    pub fn remove(_config: &AppConfig, provider: &str) -> std::result::Result<bool, KeyStoreError> {
        let keys = vault::session()?;
        let store = Vault::open()?;
        store.transaction(|| {
            let mut removed = false;
            for kind in ["api_key", "api_key_owned", "oauth"] {
                removed |= store.remove(kind, provider, &keys)?;
            }
            Ok(removed)
        })
    }

    pub fn get_oauth(
        _config: &AppConfig,
        provider: &str,
    ) -> std::result::Result<Option<OAuthCredential>, KeyStoreError> {
        let keys = vault::session()?;
        let Some(payload) = Vault::open()?.read("oauth", provider, &keys)? else {
            return Ok(None);
        };
        let credential = OAuthCredential::from_payload(
            serde_json::from_slice(&payload).map_err(|_| KeyStoreError::InvalidOauthPayload)?,
        )?;
        if credential.provider != provider {
            return Err(KeyStoreError::ProviderMismatch);
        }
        Ok(Some(credential))
    }

    pub fn upsert_oauth(
        _config: &AppConfig,
        credential: &OAuthCredential,
    ) -> std::result::Result<(), KeyStoreError> {
        Self::write_oauth(credential, None)
    }

    pub(crate) fn upsert_oauth_with_storage(
        _config: &AppConfig,
        credential: &OAuthCredential,
        storage: CredentialStorage,
    ) -> std::result::Result<(), KeyStoreError> {
        Self::write_oauth(credential, Some(storage))
    }

    fn write_oauth(
        credential: &OAuthCredential,
        storage: Option<CredentialStorage>,
    ) -> std::result::Result<(), KeyStoreError> {
        credential.validate()?;
        let keys = vault::session()?;
        let store = Vault::open()?;
        let payload = Zeroizing::new(
            serde_json::to_vec(&StoredOAuthCredential::from(credential))
                .map_err(|_| KeyStoreError::Serialize)?,
        );
        store.transaction(|| {
            store.write(
                "oauth",
                &credential.provider,
                &credential.provider,
                &payload,
                store.selected_storage(&credential.provider, storage)?,
                &keys,
            )
        })
    }

    /// Replaces only the credential a refresh actually used, preserving its storage.
    pub(crate) fn replace_oauth_if_current(
        _config: &AppConfig,
        expected: &OAuthCredential,
        replacement: &OAuthCredential,
        is_current: impl FnOnce() -> bool,
    ) -> std::result::Result<bool, KeyStoreError> {
        replacement.validate()?;
        if replacement.provider != expected.provider {
            return Err(KeyStoreError::ProviderMismatch);
        }
        let keys = vault::session()?;
        let store = Vault::open()?;
        store.transaction(|| {
            if !is_current() {
                return Ok(false);
            }
            let Some(payload) = store.read("oauth", &expected.provider, &keys)? else {
                return Ok(false);
            };
            let current = OAuthCredential::from_payload(
                serde_json::from_slice(&payload).map_err(|_| KeyStoreError::InvalidOauthPayload)?,
            )?;
            if current != *expected {
                return Ok(false);
            }
            let storage = store
                .provider_storage(&expected.provider)?
                .ok_or(KeyStoreError::InvalidOauthPayload)?;
            let payload = Zeroizing::new(
                serde_json::to_vec(&StoredOAuthCredential::from(replacement))
                    .map_err(|_| KeyStoreError::Serialize)?,
            );
            store.write(
                "oauth",
                &replacement.provider,
                &replacement.provider,
                &payload,
                storage,
                &keys,
            )?;
            Ok(true)
        })
    }

    pub fn remove_oauth(
        _config: &AppConfig,
        provider: &str,
    ) -> std::result::Result<bool, KeyStoreError> {
        let keys = vault::session()?;
        Vault::open()?.remove("oauth", provider, &keys)
    }

    pub fn list_secrets(
        _config: &AppConfig,
    ) -> std::result::Result<Vec<SecretEntry>, KeyStoreError> {
        let keys = vault::session()?;
        let store = Vault::open()?;
        store.require_key(&keys)?;
        let mut entries = Vec::new();
        for record in store.records()? {
            let kind = match record.kind.as_str() {
                "machine_password" => SecretKind::Password,
                "ssh_key" => SecretKind::SshKey,
                _ => continue,
            };
            // Authenticate labels before displaying potentially modified metadata.
            store.read(&record.kind, &record.id, &keys)?;
            entries.push(SecretEntry {
                id: record.id,
                label: record.label,
                kind,
            });
        }
        Ok(entries)
    }

    pub fn set_secret(
        _config: &AppConfig,
        id: &str,
        label: &str,
        kind: SecretKind,
        secret: &str,
    ) -> std::result::Result<(), KeyStoreError> {
        if label.trim().is_empty() {
            return Err(KeyStoreError::InvalidCredentialPayload);
        }
        let keys = vault::session()?;
        let store = Vault::open()?;
        store.transaction(|| {
            // IDs are unique across the two machine-secret types.
            for other in [SecretKind::Password, SecretKind::SshKey] {
                if other != kind {
                    store.remove(other.record_kind(), id, &keys)?;
                }
            }
            store.write(
                kind.record_kind(),
                id,
                label,
                secret.as_bytes(),
                CredentialStorage::Encrypted,
                &keys,
            )
        })
    }

    pub fn get_secret(
        _config: &AppConfig,
        id: &str,
    ) -> std::result::Result<Option<String>, KeyStoreError> {
        let keys = vault::session()?;
        let store = Vault::open()?;
        store.require_key(&keys)?;
        for kind in [SecretKind::Password, SecretKind::SshKey] {
            if let Some(payload) = store.read(kind.record_kind(), id, &keys)? {
                return Ok(Some(
                    std::str::from_utf8(&payload)
                        .map_err(|_| KeyStoreError::InvalidCredentialPayload)?
                        .to_owned(),
                ));
            }
        }
        Ok(None)
    }

    pub fn remove_secret(
        _config: &AppConfig,
        id: &str,
    ) -> std::result::Result<bool, KeyStoreError> {
        let keys = vault::session()?;
        let store = Vault::open()?;
        store.require_key(&keys)?;
        store.transaction(|| {
            let mut removed = false;
            for kind in [SecretKind::Password, SecretKind::SshKey] {
                removed |= store.remove(kind.record_kind(), id, &keys)?;
            }
            Ok(removed)
        })
    }

    /// Imports the legacy file only when explicitly requested after unlocking.
    /// The source file and machine key are preserved so failed imports never lose data.
    pub fn migrate_legacy(config: &AppConfig) -> std::result::Result<usize, KeyStoreError> {
        use crate::storage::crypto::{decrypt_serialized, decrypt_shared_text_with_key, SharedKey};
        let keys = vault::session()?;
        let store = Vault::open()?;
        store.require_key(&keys)?;
        let file = persistence::read_file(&Self::keys_path(config)?)?.into_current()?;
        let needs_machine_key = !file.oauth.is_empty()
            || !file.credentials.is_empty()
            || file.keys.values().any(|value| value.starts_with("enc:"));
        let machine_key = if needs_machine_key {
            Some(
                SharedKey::load_from_path(
                    &crate::storage::paths::TcuiDataPaths::discover().chat_key,
                )
                .map_err(|_| KeyStoreError::KeyAccess)?,
            )
        } else {
            None
        };
        let mut records = Vec::new();
        for (provider, stored) in file.keys {
            let plaintext = if let Some(key) = &machine_key {
                decrypt_shared_text_with_key(key, &stored)
                    .map_err(|_| KeyStoreError::CredentialDecrypt)?
            } else {
                stored
            };
            records.push(("api_key", provider, Zeroizing::new(plaintext.into_bytes())));
        }
        for (provider, stored) in file.credentials {
            let payload: StoredApiKeyCredential = decrypt_serialized(
                machine_key.as_ref().ok_or(KeyStoreError::KeyAccess)?,
                &format!("credential:api_key:{provider}"),
                &stored,
            )
            .map_err(|_| KeyStoreError::CredentialDecrypt)?;
            let credential = ApiKeyCredential::from_payload(payload)?;
            if credential.provider() != provider {
                return Err(KeyStoreError::ProviderMismatch);
            }
            // The OAuth preset has a distinct identity from manually supplied API keys.
            let (provider, credential) = if provider == "OpenRouter"
                && credential.source() == ApiKeyCredentialSource::OpenRouterPkce
            {
                let migrated = ApiKeyCredential::new(
                    "OpenRouter OAuth",
                    credential.api_key(),
                    ApiKeyCredentialOwnership::Tcui,
                    credential.source(),
                )?;
                ("OpenRouter OAuth".to_owned(), migrated)
            } else {
                (provider, credential)
            };
            records.push((
                "api_key_owned",
                provider,
                Zeroizing::new(
                    serde_json::to_vec(&StoredApiKeyCredential::from(&credential))
                        .map_err(|_| KeyStoreError::Serialize)?,
                ),
            ));
        }
        for (provider, stored) in file.oauth {
            let payload: StoredOAuthCredential = decrypt_serialized(
                machine_key.as_ref().ok_or(KeyStoreError::KeyAccess)?,
                &format!("oauth:{provider}"),
                &stored,
            )
            .map_err(|_| KeyStoreError::OauthDecrypt)?;
            let credential = OAuthCredential::from_payload(payload)?;
            if credential.provider != provider {
                return Err(KeyStoreError::ProviderMismatch);
            }
            records.push((
                "oauth",
                provider,
                Zeroizing::new(
                    serde_json::to_vec(&StoredOAuthCredential::from(&credential))
                        .map_err(|_| KeyStoreError::Serialize)?,
                ),
            ));
        }
        store.transaction(|| {
            let mut count = 0;
            for (kind, id, payload) in &records {
                if let Some(existing) = store.read(kind, id, &keys)? {
                    if existing.as_slice() != payload.as_slice() {
                        return Err(KeyStoreError::MigrationConflict);
                    }
                    continue;
                }
                store.write(kind, id, id, payload, CredentialStorage::Encrypted, &keys)?;
                count += 1;
            }
            Ok(count)
        })
    }

    fn keys_path(config: &AppConfig) -> std::result::Result<PathBuf, KeyStoreError> {
        Ok(config
            .key_file
            .as_deref()
            .map(str::trim)
            .filter(|path| !path.is_empty())
            .map(PathBuf::from)
            .unwrap_or_else(|| crate::storage::paths::TcuiDataPaths::discover().keys_file))
    }
}

impl Default for KeyStore {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod credential_tests;
#[cfg(test)]
mod filesystem_tests;
#[cfg(test)]
mod future_version_tests;
#[cfg(test)]
mod legacy_tests;
#[cfg(test)]
mod oauth_api_tests;
#[cfg(test)]
mod oauth_concurrency_tests;
#[cfg(test)]
mod oauth_persistence_tests;
#[cfg(test)]
mod parent_security_tests;
#[cfg(test)]
pub(crate) mod test_support;
