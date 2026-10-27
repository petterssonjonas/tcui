use std::fmt;

use serde::{Deserialize, Serialize};

use super::KeyStoreError;

#[derive(Clone, PartialEq, Eq)]
pub enum Credential {
    ApiKey(ApiKeyCredential),
}

impl fmt::Debug for Credential {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ApiKey(credential) => formatter
                .debug_tuple("Credential::ApiKey")
                .field(credential)
                .finish(),
        }
    }
}

impl Credential {
    pub fn as_api_key(&self) -> &str {
        match self {
            Self::ApiKey(credential) => credential.api_key(),
        }
    }
}

#[derive(Clone, PartialEq, Eq)]
pub struct ApiKeyCredential {
    provider: String,
    api_key: String,
    ownership: ApiKeyCredentialOwnership,
    source: ApiKeyCredentialSource,
}

impl ApiKeyCredential {
    pub fn new(
        provider: impl Into<String>,
        api_key: impl Into<String>,
        ownership: ApiKeyCredentialOwnership,
        source: ApiKeyCredentialSource,
    ) -> Result<Self, KeyStoreError> {
        let credential = Self {
            provider: provider.into(),
            api_key: api_key.into(),
            ownership,
            source,
        };
        credential.validate()?;
        Ok(credential)
    }

    pub fn api_key(&self) -> &str {
        &self.api_key
    }

    pub fn source(&self) -> ApiKeyCredentialSource {
        self.source
    }

    pub(super) fn provider(&self) -> &str {
        &self.provider
    }

    fn validate(&self) -> Result<(), KeyStoreError> {
        if self.provider.trim().is_empty() || self.api_key.trim().is_empty() {
            return Err(KeyStoreError::InvalidCredential);
        }
        Ok(())
    }

    pub(super) fn from_payload(payload: StoredApiKeyCredential) -> Result<Self, KeyStoreError> {
        Self::new(
            payload.provider,
            payload.api_key,
            payload.ownership,
            payload.source,
        )
        .map_err(|_| KeyStoreError::InvalidCredentialPayload)
    }
}

impl fmt::Debug for ApiKeyCredential {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ApiKeyCredential")
            .field("provider", &self.provider)
            .field("api_key", &"<redacted>")
            .field("ownership", &self.ownership)
            .field("source", &self.source)
            .finish()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ApiKeyCredentialOwnership {
    Tcui,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ApiKeyCredentialSource {
    OpenRouterPkce,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct StoredApiKeyCredential {
    provider: String,
    api_key: String,
    ownership: ApiKeyCredentialOwnership,
    source: ApiKeyCredentialSource,
}

impl From<&ApiKeyCredential> for StoredApiKeyCredential {
    fn from(credential: &ApiKeyCredential) -> Self {
        Self {
            provider: credential.provider.clone(),
            api_key: credential.api_key.clone(),
            ownership: credential.ownership,
            source: credential.source,
        }
    }
}
