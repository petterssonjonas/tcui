use crate::config::AppConfig;
use crate::storage::crypto::{decrypt_serialized, encrypt_serialized, StorageCryptoError};

use super::credential::StoredApiKeyCredential;
use super::{ApiKeyCredential, ApiKeyCredentialSource, Credential, KeyStore, KeyStoreError};

impl KeyStore {
    pub fn get_api_key_credential(
        config: &AppConfig,
        provider: &str,
    ) -> std::result::Result<Option<ApiKeyCredential>, KeyStoreError> {
        let file = Self::load_file(config)?.into_current()?;
        let Some(encrypted) = file.credentials.get(provider) else {
            return Ok(None);
        };
        let payload = decrypt_serialized::<StoredApiKeyCredential>(
            &super::shared_key()?,
            &credential_record_kind(provider),
            encrypted,
        )
        .map_err(map_credential_crypto_error)?;
        let credential = ApiKeyCredential::from_payload(payload)?;
        if credential.provider() != provider {
            return Err(KeyStoreError::ProviderMismatch);
        }
        Ok(Some(credential))
    }

    pub fn get_credential(
        config: &AppConfig,
        provider: &str,
    ) -> std::result::Result<Option<Credential>, KeyStoreError> {
        Self::get_api_key_credential(config, provider)
            .map(|credential| credential.map(Credential::ApiKey))
    }

    pub fn upsert_credential(
        config: &AppConfig,
        credential: &Credential,
    ) -> std::result::Result<(), KeyStoreError> {
        match credential {
            Credential::ApiKey(credential) => Self::upsert_api_key(config, credential),
        }
    }

    pub fn remove_api_key(
        config: &AppConfig,
        provider: &str,
        source: ApiKeyCredentialSource,
    ) -> std::result::Result<bool, KeyStoreError> {
        let _guard = super::store_write_guard()?;
        let Some(credential) = Self::get_api_key_credential(config, provider)? else {
            return Ok(false);
        };
        if credential.source() != source {
            return Ok(false);
        }
        let mut file = Self::load_file(config)?.into_current()?;
        file.credentials.remove(provider);
        Self::write_file(config, &file)?;
        Ok(true)
    }

    fn upsert_api_key(
        config: &AppConfig,
        credential: &ApiKeyCredential,
    ) -> std::result::Result<(), KeyStoreError> {
        let _guard = super::store_write_guard()?;
        let mut file = Self::load_file(config)?.into_current()?;
        let encrypted = encrypt_serialized(
            &super::shared_key()?,
            &credential_record_kind(credential.provider()),
            &StoredApiKeyCredential::from(credential),
        )
        .map_err(map_credential_crypto_error)?;
        file.credentials
            .insert(credential.provider().to_owned(), encrypted);
        Self::write_file(config, &file)
    }
}

fn credential_record_kind(provider: &str) -> String {
    format!("credential:api_key:{provider}")
}

fn map_credential_crypto_error(error: StorageCryptoError) -> KeyStoreError {
    match error {
        StorageCryptoError::Encrypt => KeyStoreError::CredentialEncrypt,
        StorageCryptoError::Json(_) | StorageCryptoError::Utf8(_) => {
            KeyStoreError::InvalidCredentialPayload
        }
        StorageCryptoError::Io(_)
        | StorageCryptoError::InvalidKeyLength
        | StorageCryptoError::MissingDefaultKey { .. } => KeyStoreError::KeyAccess,
        StorageCryptoError::Base64(_)
        | StorageCryptoError::MissingNonce
        | StorageCryptoError::MissingCiphertext
        | StorageCryptoError::InvalidEnvelope
        | StorageCryptoError::Decrypt
        | StorageCryptoError::WrongDocumentKind => KeyStoreError::CredentialDecrypt,
    }
}
