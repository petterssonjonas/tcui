use super::credential::StoredApiKeyCredential;
use super::vault::{self, Vault};
use super::{ApiKeyCredential, ApiKeyCredentialSource, Credential, KeyStore, KeyStoreError};
use crate::config::AppConfig;
use zeroize::Zeroizing;

impl KeyStore {
    pub fn get_api_key_credential(
        _config: &AppConfig,
        provider: &str,
    ) -> std::result::Result<Option<ApiKeyCredential>, KeyStoreError> {
        let keys = vault::session()?;
        let Some(payload) = Vault::open()?.read("api_key_owned", provider, &keys)? else {
            return Ok(None);
        };
        let credential = ApiKeyCredential::from_payload(
            serde_json::from_slice(&payload)
                .map_err(|_| KeyStoreError::InvalidCredentialPayload)?,
        )?;
        if credential.provider() != provider {
            return Err(KeyStoreError::ProviderMismatch);
        }
        Ok(Some(credential))
    }

    #[cfg(test)]
    pub fn get_credential(
        config: &AppConfig,
        provider: &str,
    ) -> std::result::Result<Option<Credential>, KeyStoreError> {
        Self::get_api_key_credential(config, provider)
            .map(|credential| credential.map(Credential::ApiKey))
    }

    pub fn upsert_credential(
        _config: &AppConfig,
        credential: &Credential,
    ) -> std::result::Result<(), KeyStoreError> {
        match credential {
            Credential::ApiKey(credential) => {
                let keys = vault::session()?;
                let store = Vault::open()?;
                let payload = Zeroizing::new(
                    serde_json::to_vec(&StoredApiKeyCredential::from(credential))
                        .map_err(|_| KeyStoreError::Serialize)?,
                );
                store.transaction(|| {
                    store.write(
                        "api_key_owned",
                        credential.provider(),
                        credential.provider(),
                        &payload,
                        store.preference()?,
                        &keys,
                    )
                })
            }
        }
    }

    pub fn remove_api_key(
        _config: &AppConfig,
        provider: &str,
        source: ApiKeyCredentialSource,
    ) -> std::result::Result<bool, KeyStoreError> {
        let keys = vault::session()?;
        let store = Vault::open()?;
        let Some(payload) = store.read("api_key_owned", provider, &keys)? else {
            return Ok(false);
        };
        let credential = ApiKeyCredential::from_payload(
            serde_json::from_slice(&payload)
                .map_err(|_| KeyStoreError::InvalidCredentialPayload)?,
        )?;
        if credential.provider() != provider {
            return Err(KeyStoreError::ProviderMismatch);
        }
        if credential.source() != source {
            return Ok(false);
        }
        store.remove("api_key_owned", provider, &keys)
    }
}
