use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::fmt;

#[derive(Clone, PartialEq, Eq)]
pub struct OAuthCredential {
    pub provider: String,
    pub access_token: String,
    pub refresh_token: Option<String>,
    pub expires_at: DateTime<Utc>,
    pub account_id: Option<String>,
    pub ownership: OAuthCredentialOwnership,
    pub source: OAuthCredentialSource,
}

impl OAuthCredential {
    pub(super) fn validate(&self) -> Result<(), KeyStoreError> {
        if self.provider.trim().is_empty() || self.access_token.trim().is_empty() {
            return Err(KeyStoreError::InvalidCredential);
        }
        if self
            .refresh_token
            .as_deref()
            .is_some_and(|token| token.trim().is_empty())
        {
            return Err(KeyStoreError::InvalidCredential);
        }
        Ok(())
    }

    pub(super) fn from_payload(payload: StoredOAuthCredential) -> Result<Self, KeyStoreError> {
        let credential = Self {
            provider: payload.provider,
            access_token: payload.access_token,
            refresh_token: payload.refresh_token,
            expires_at: payload.expires_at,
            account_id: payload.account_id,
            ownership: payload.ownership,
            source: payload.source,
        };
        credential
            .validate()
            .map_err(|_| KeyStoreError::InvalidOauthPayload)?;
        Ok(credential)
    }
}

impl fmt::Debug for OAuthCredential {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("OAuthCredential")
            .field("provider", &self.provider)
            .field("access_token", &"<redacted>")
            .field("refresh_token_present", &self.refresh_token.is_some())
            .field("expires_at", &self.expires_at)
            .field("account_id_present", &self.account_id.is_some())
            .field("ownership", &self.ownership)
            .field("source", &self.source)
            .finish()
    }
}

impl fmt::Display for OAuthCredential {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "OAuthCredential(provider={}, access_token=<redacted>, refresh_token_present={}, expires_at={}, account_id_present={}, ownership={:?}, source={:?})",
            self.provider,
            self.refresh_token.is_some(),
            self.expires_at,
            self.account_id.is_some(),
            self.ownership,
            self.source
        )
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OAuthCredentialOwnership {
    Tcui,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OAuthCredentialSource {
    NativeOAuth,
}

#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum KeyStoreError {
    #[error("credential store path is unavailable")]
    KeyPath,
    #[error("credential store directory could not be created")]
    CreateDirectory,
    #[error("credential store could not be read")]
    Read,
    #[error("credential store path is unsafe")]
    UnsafePath,
    #[error("credential store could not be parsed")]
    Parse,
    #[error("credential store version {version} is unsupported")]
    UnsupportedVersion { version: u32 },
    #[error("credential store could not be serialized")]
    Serialize,
    #[error("credential store could not be written")]
    Write,
    #[error("local encryption key is unavailable")]
    KeyAccess,
    #[error("OAuth credential is invalid")]
    InvalidCredential,
    #[error("OAuth credential could not be decrypted")]
    OauthDecrypt,
    #[error("OAuth credential payload is invalid")]
    InvalidOauthPayload,
    #[error("OAuth credential provider does not match its storage key")]
    ProviderMismatch,
    #[error("credential could not be encrypted")]
    CredentialEncrypt,
    #[error("credential could not be decrypted")]
    CredentialDecrypt,
    #[error("credential payload is invalid")]
    InvalidCredentialPayload,
    #[error("credential vault is locked; unlock it with /vault")]
    VaultLocked,
    #[error("credential vault is not initialized")]
    VaultNotInitialized,
    #[error("credential vault is already initialized")]
    VaultAlreadyInitialized,
    #[error("vault password is empty")]
    EmptyPassword,
    #[error("choose a new password that differs from the current password")]
    PasswordUnchanged,
    #[error("vault password is incorrect or vault data was modified")]
    VaultAuthentication,
    #[error("legacy credential conflicts with an existing credential")]
    MigrationConflict,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct StoredOAuthCredential {
    provider: String,
    access_token: String,
    refresh_token: Option<String>,
    expires_at: DateTime<Utc>,
    account_id: Option<String>,
    ownership: OAuthCredentialOwnership,
    source: OAuthCredentialSource,
}

impl From<&OAuthCredential> for StoredOAuthCredential {
    fn from(credential: &OAuthCredential) -> Self {
        Self {
            provider: credential.provider.clone(),
            access_token: credential.access_token.clone(),
            refresh_token: credential.refresh_token.clone(),
            expires_at: credential.expires_at,
            account_id: credential.account_id.clone(),
            ownership: credential.ownership,
            source: credential.source,
        }
    }
}
