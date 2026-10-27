mod browser;
mod device;
mod endpoints;
mod error;
mod revoke;
mod store;

use std::future::Future;
use std::time::Duration;

use crate::config::AppConfig;
use crate::llm::auth::oauth::{BrowserLauncher, OAuthCancellation};

pub(crate) use device::CodexDeviceAuthorization;
pub(crate) use error::CodexNativeError;

use endpoints::CodexNativeEndpoints;

const NATIVE_HTTP_TIMEOUT: Duration = Duration::from_secs(30);
const NATIVE_CALLBACK_TIMEOUT: Duration = Duration::from_secs(5 * 60);
const NATIVE_DEVICE_TIMEOUT: Duration = Duration::from_secs(15 * 60);
const NATIVE_REVOCATION_TIMEOUT: Duration = Duration::from_secs(10);

#[derive(Clone)]
pub(crate) struct CodexNativeAdapter {
    endpoints: CodexNativeEndpoints,
    revocation_timeout: Duration,
    connection: String,
    storage: Option<crate::config::key_store::CredentialStorage>,
}

impl CodexNativeAdapter {
    pub(crate) fn production() -> Result<Self, CodexNativeError> {
        Ok(Self {
            endpoints: CodexNativeEndpoints::production()?,
            revocation_timeout: NATIVE_REVOCATION_TIMEOUT,
            connection: "Codex".into(),
            storage: None,
        })
    }

    #[cfg(any(test, debug_assertions))]
    pub(crate) fn fixture(
        authorization: &str,
        token: &str,
        device_user_code: &str,
        device_token: &str,
    ) -> Result<Self, CodexNativeError> {
        Ok(Self {
            endpoints: CodexNativeEndpoints::fixture(
                authorization,
                token,
                device_user_code,
                device_token,
            )?,
            revocation_timeout: NATIVE_REVOCATION_TIMEOUT,
            connection: "Codex".into(),
            storage: None,
        })
    }

    #[cfg(test)]
    pub(crate) fn fixture_with_revocation(
        authorization: &str,
        token: &str,
        device_user_code: &str,
        device_token: &str,
        revocation: &str,
        revocation_timeout: Duration,
    ) -> Result<Self, CodexNativeError> {
        Ok(Self {
            endpoints: CodexNativeEndpoints::fixture_with_revocation(
                authorization,
                token,
                device_user_code,
                device_token,
                revocation,
            )?,
            revocation_timeout,
            connection: "Codex".into(),
            storage: None,
        })
    }

    pub(crate) fn for_connection(mut self, connection: &str) -> Self {
        self.connection = connection.to_owned();
        self
    }

    pub(crate) fn with_storage(
        mut self,
        storage: crate::config::key_store::CredentialStorage,
    ) -> Self {
        self.storage = Some(storage);
        self
    }

    pub(crate) async fn login_browser(
        &self,
        config: &AppConfig,
        browser: &impl BrowserLauncher,
        cancellation: &OAuthCancellation,
        input: impl Future<
            Output = Result<zeroize::Zeroizing<String>, crate::llm::auth::oauth::OAuthError>,
        >,
    ) -> Result<super::credential::CodexCredential, CodexNativeError> {
        browser::login(self, config, browser, cancellation, input).await
    }

    pub(crate) async fn begin_device(
        &self,
        cancellation: &OAuthCancellation,
    ) -> Result<CodexDeviceAuthorization, CodexNativeError> {
        device::begin(self, cancellation).await
    }

    pub(crate) async fn complete_device(
        &self,
        config: &AppConfig,
        authorization: CodexDeviceAuthorization,
        cancellation: &OAuthCancellation,
    ) -> Result<super::credential::CodexCredential, CodexNativeError> {
        device::complete(self, config, authorization, cancellation).await
    }

    pub(crate) async fn refresh(
        &self,
        config: &AppConfig,
        cancellation: &OAuthCancellation,
        is_current: impl FnOnce() -> bool,
    ) -> Result<super::credential::CodexCredential, CodexNativeError> {
        store::refresh(self, config, cancellation, is_current).await
    }

    pub(crate) async fn logout(
        &self,
        config: &AppConfig,
        cancellation: &OAuthCancellation,
    ) -> Result<CodexNativeLogout, CodexNativeError> {
        let Some(credential) = crate::config::KeyStore::get_oauth(config, &self.connection)? else {
            return Ok(CodexNativeLogout::NoNativeCredential);
        };
        let outcome = revoke::revoke(self, &credential, cancellation).await;
        crate::llm::auth::invalidate_provider_refreshes(&self.connection);
        let removed = crate::config::KeyStore::remove_oauth(config, &self.connection)?;
        if !removed {
            return Err(CodexNativeError::LocalRemoval);
        }
        Ok(match outcome {
            Ok(()) => CodexNativeLogout::Revoked,
            Err(failure) => CodexNativeLogout::RevocationFailed(failure),
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CodexNativeLogout {
    NoNativeCredential,
    Revoked,
    RevocationFailed(CodexRevocationFailure),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CodexRevocationFailure {
    Cancelled,
    TimedOut,
    Rejected,
    Transport,
    MalformedResponse,
}
