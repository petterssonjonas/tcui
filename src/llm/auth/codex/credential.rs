use std::fmt;
use std::io::Read;
use std::path::Path;

use chrono::{DateTime, Utc};
use secrecy::SecretString;
use serde::Deserialize;

use crate::config::key_store::{
    KeyStoreError, OAuthCredential, OAuthCredentialOwnership, OAuthCredentialSource,
};
use crate::config::{AppConfig, KeyStore};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum CodexCredentialSource {
    ExternalCli,
    TcuiNative,
}

#[derive(Clone)]
pub(crate) struct CodexCredential {
    access_token: SecretString,
    refresh_token: Option<SecretString>,
    account_id: Option<String>,
    expires_at: Option<DateTime<Utc>>,
    source: CodexCredentialSource,
}

impl CodexCredential {
    fn external(
        access_token: String,
        refresh_token: Option<String>,
        account_id: Option<String>,
    ) -> Self {
        Self {
            access_token: SecretString::from(access_token),
            refresh_token: refresh_token.map(SecretString::from),
            account_id,
            expires_at: None,
            source: CodexCredentialSource::ExternalCli,
        }
    }

    pub(crate) fn native(credential: OAuthCredential) -> Self {
        Self {
            access_token: SecretString::from(credential.access_token),
            refresh_token: credential.refresh_token.map(SecretString::from),
            account_id: credential.account_id,
            expires_at: Some(credential.expires_at),
            source: CodexCredentialSource::TcuiNative,
        }
    }

    pub(crate) fn access_token(&self) -> &SecretString {
        &self.access_token
    }

    pub(crate) fn account_id(&self) -> Option<&str> {
        self.account_id.as_deref()
    }

    pub(crate) const fn source(&self) -> CodexCredentialSource {
        self.source
    }

    pub(crate) fn expires_at(&self) -> Option<DateTime<Utc>> {
        self.expires_at
    }
}

impl fmt::Debug for CodexCredential {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("CodexCredential")
            .field("access_token", &"<redacted>")
            .field("refresh_token_present", &self.refresh_token.is_some())
            .field("account_id_present", &self.account_id.is_some())
            .field("expires_at", &self.expires_at)
            .field("source", &self.source)
            .finish()
    }
}

impl fmt::Display for CodexCredential {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "CodexCredential(access_token=<redacted>, refresh_token_present={}, account_id_present={}, expires_at={:?}, source={:?})",
            self.refresh_token.is_some(),
            self.account_id.is_some(),
            self.expires_at,
            self.source
        )
    }
}

#[derive(Debug, thiserror::Error)]
pub(crate) enum CodexCredentialError {
    #[error("Codex CLI credentials could not be read")]
    Read,
    #[error("Codex CLI credentials are malformed; run `codex login` or choose `--native`")]
    Malformed,
    #[error("Codex CLI credentials are stored in an unsafe file")]
    UnsafeFile,
}

pub(crate) fn read_external_credential() -> Result<Option<CodexCredential>, CodexCredentialError> {
    let Some(home) = dirs::home_dir() else {
        return Ok(None);
    };
    for path in [
        home.join(".codex").join("auth.json"),
        home.join(".codex.json"),
    ] {
        match read_external_credential_path(&path) {
            Ok(Some(credential)) => return Ok(Some(credential)),
            Ok(None) => {}
            Err(CodexCredentialError::Malformed) => return Err(CodexCredentialError::Malformed),
            Err(CodexCredentialError::Read) => return Err(CodexCredentialError::Read),
            Err(CodexCredentialError::UnsafeFile) => return Err(CodexCredentialError::UnsafeFile),
        }
    }
    Ok(None)
}

pub(crate) fn resolve_credential(
    config: &AppConfig,
) -> Result<Option<CodexCredential>, CodexResolutionError> {
    if let Some(credential) = KeyStore::get_oauth(config, "Codex")? {
        if credential.ownership != OAuthCredentialOwnership::Tcui
            || credential.source != OAuthCredentialSource::NativeOAuth
        {
            return Err(CodexResolutionError::UnexpectedNativeCredential);
        }
        return Ok(Some(CodexCredential::native(credential)));
    }
    read_external_credential().map_err(CodexResolutionError::External)
}

#[derive(Debug, thiserror::Error)]
pub(crate) enum CodexResolutionError {
    #[error("TCUI Codex credential metadata is invalid")]
    UnexpectedNativeCredential,
    #[error("TCUI Codex credential store is unavailable")]
    Store(#[from] KeyStoreError),
    #[error("Codex CLI credentials are unavailable")]
    External(#[from] CodexCredentialError),
}

fn read_external_credential_path(
    path: &Path,
) -> Result<Option<CodexCredential>, CodexCredentialError> {
    let file = match std::fs::File::open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(_) => return Err(CodexCredentialError::Read),
    };
    validate_external_auth_file(path, &file)?;
    let mut contents = String::new();
    let mut file = file;
    file.read_to_string(&mut contents)
        .map_err(|_| CodexCredentialError::Read)?;
    let stored = serde_json::from_str::<StoredCodexAuth>(&contents)
        .map_err(|_| CodexCredentialError::Malformed)?;
    stored
        .credential()
        .map(Some)
        .ok_or(CodexCredentialError::Malformed)
}

fn validate_external_auth_file(
    path: &Path,
    file: &std::fs::File,
) -> Result<(), CodexCredentialError> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;

        let path_metadata =
            std::fs::symlink_metadata(path).map_err(|_| CodexCredentialError::Read)?;
        if !path_metadata.file_type().is_file() {
            return Err(CodexCredentialError::UnsafeFile);
        }
        let metadata = file.metadata().map_err(|_| CodexCredentialError::Read)?;
        let Some(home) = dirs::home_dir() else {
            return Err(CodexCredentialError::Read);
        };
        let home_metadata = std::fs::metadata(home).map_err(|_| CodexCredentialError::Read)?;
        if !external_metadata_is_safe(metadata.uid(), metadata.mode(), home_metadata.uid()) {
            return Err(CodexCredentialError::UnsafeFile);
        }
    }
    #[cfg(not(unix))]
    {
        let _ = (path, file);
    }
    Ok(())
}

pub(crate) const fn external_metadata_is_safe(owner: u32, mode: u32, expected_owner: u32) -> bool {
    owner == expected_owner && mode & 0o077 == 0
}

#[derive(Deserialize)]
struct StoredCodexAuth {
    #[serde(default)]
    account: Option<String>,
    #[serde(default)]
    tokens: Option<StoredTokenSet>,
    #[serde(default)]
    oauth_token_set: Option<StoredTokenSet>,
    #[serde(default)]
    codex_access_token: Option<StoredAccessToken>,
}

impl StoredCodexAuth {
    fn credential(self) -> Option<CodexCredential> {
        let account = self.account.and_then(non_empty);
        self.tokens
            .and_then(|tokens| tokens.credential(account.clone()))
            .or_else(|| {
                self.oauth_token_set
                    .and_then(|tokens| tokens.credential(account.clone()))
            })
            .or_else(|| {
                self.codex_access_token
                    .and_then(|token| token.credential(account))
            })
    }
}

#[derive(Deserialize)]
struct StoredTokenSet {
    access_token: Option<String>,
    #[serde(default)]
    refresh_token: Option<String>,
    #[serde(default)]
    account_id: Option<String>,
}

impl StoredTokenSet {
    fn credential(self, account: Option<String>) -> Option<CodexCredential> {
        let access_token = self.access_token.and_then(non_empty)?;
        let refresh_token = self.refresh_token.and_then(non_empty);
        let account_id = self.account_id.and_then(non_empty).or(account);
        Some(CodexCredential::external(
            access_token,
            refresh_token,
            account_id,
        ))
    }
}

#[derive(Deserialize)]
struct StoredAccessToken {
    access_token: Option<String>,
    #[serde(default)]
    account: Option<String>,
}

impl StoredAccessToken {
    fn credential(self, account: Option<String>) -> Option<CodexCredential> {
        let access_token = self.access_token.and_then(non_empty)?;
        Some(CodexCredential::external(
            access_token,
            None,
            self.account.and_then(non_empty).or(account),
        ))
    }
}

fn non_empty(value: String) -> Option<String> {
    let value = value.trim();
    (!value.is_empty()).then(|| value.to_string())
}
