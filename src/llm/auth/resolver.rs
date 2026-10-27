use std::sync::OnceLock;

use chrono::Utc;
use tokio::sync::{oneshot, Mutex};

use crate::config::{AppConfig, KeyStore};

use super::codex::{CodexCredential, CodexCredentialSource, CodexNativeAdapter};
use super::credential::Credential;
use super::oauth::oauth_cancellation;
use super::policy::{canonical_provider_name, is_oauth_provider, trusted_provider_endpoint};
use super::reader::{read_env_file, read_oauth_token};
use super::{CredentialError, CredentialSource};

pub(crate) struct CredentialRequest<'a> {
    provider: &'a str,
    env_var: &'a str,
    endpoint: &'a str,
}

impl<'a> CredentialRequest<'a> {
    pub(crate) const fn new(provider: &'a str, env_var: &'a str, endpoint: &'a str) -> Self {
        Self {
            provider,
            env_var,
            endpoint,
        }
    }
}

type NativeRefreshResult = Result<CodexCredential, CredentialError>;
type NativeRefreshWaiter = oneshot::Sender<NativeRefreshResult>;

fn native_refresh_flight() -> &'static Mutex<Option<Vec<NativeRefreshWaiter>>> {
    static FLIGHT: OnceLock<Mutex<Option<Vec<NativeRefreshWaiter>>>> = OnceLock::new();
    FLIGHT.get_or_init(|| Mutex::new(None))
}

pub(crate) async fn resolve_provider_credential(
    request: CredentialRequest<'_>,
) -> Result<Option<Credential>, CredentialError> {
    resolve_with_config(request, AppConfig::load().ok(), None).await
}

#[cfg(test)]
pub(crate) async fn resolve_provider_credential_with_native_adapter(
    request: CredentialRequest<'_>,
    config: &AppConfig,
    adapter: &CodexNativeAdapter,
) -> Result<Option<Credential>, CredentialError> {
    resolve_with_config(request, Some(config.clone()), Some(adapter)).await
}

#[cfg(test)]
pub(crate) async fn resolve_provider_credential_with_config(
    request: CredentialRequest<'_>,
    config: &AppConfig,
) -> Result<Option<Credential>, CredentialError> {
    resolve_with_config(request, Some(config.clone()), None).await
}

async fn resolve_with_config(
    request: CredentialRequest<'_>,
    config: Option<AppConfig>,
    native_adapter: Option<&CodexNativeAdapter>,
) -> Result<Option<Credential>, CredentialError> {
    if !trusted_provider_endpoint(request.provider, request.endpoint) {
        return Err(CredentialError::UntrustedEndpoint);
    }

    let provider = canonical_provider_name(request.provider);
    if provider == "Codex" {
        let config = config.ok_or(CredentialError::CodexConfiguration)?;
        return resolve_codex(&config, native_adapter)
            .await
            .map(|credential| credential.map(Credential::codex));
    }

    if let Some(key) = std::env::var(request.env_var)
        .ok()
        .filter(|value| !value.trim().is_empty())
    {
        return Ok(Some(Credential::api_key(
            provider,
            CredentialSource::Environment,
            key,
        )));
    }
    if let Some(key) = read_env_file(request.env_var) {
        return Ok(Some(Credential::api_key(
            provider,
            CredentialSource::DotEnv,
            key,
        )));
    }

    if !is_oauth_provider(request.provider) {
        if let Some(config) = config.as_ref() {
            if let Some(key) = KeyStore::get(config, &provider)
                .ok()
                .flatten()
                .filter(|value| !value.trim().is_empty())
            {
                return Ok(Some(Credential::api_key(
                    provider,
                    CredentialSource::LegacyKeyStore,
                    key,
                )));
            }
            if let Some(credential) = KeyStore::get_api_key_credential(config, &provider)
                .ok()
                .flatten()
            {
                return Ok(Some(Credential::api_key(
                    provider,
                    CredentialSource::TcuiStoredApiKey,
                    credential.api_key().to_owned(),
                )));
            }
        }
    }

    Ok(read_oauth_token(request.provider)
        .map(|token| Credential::api_key(provider, CredentialSource::PassiveExternalToken, token)))
}

async fn resolve_codex(
    config: &AppConfig,
    native_adapter: Option<&CodexNativeAdapter>,
) -> Result<Option<CodexCredential>, CredentialError> {
    let credential = super::codex::resolve_credential(config)
        .map_err(|_| CredentialError::CodexCredentialUnavailable)?;
    let Some(credential) = credential else {
        return Ok(None);
    };
    if credential.source() == CodexCredentialSource::ExternalCli || !is_expired(&credential) {
        return Ok(Some(credential));
    }

    refresh_native_singleflight(config, native_adapter)
        .await
        .map(Some)
}

fn is_expired(credential: &CodexCredential) -> bool {
    credential
        .expires_at()
        .is_some_and(|expiry| expiry <= Utc::now())
}

async fn refresh_native_singleflight(
    config: &AppConfig,
    native_adapter: Option<&CodexNativeAdapter>,
) -> NativeRefreshResult {
    let (sender, receiver) = oneshot::channel();
    let starts_refresh = {
        let mut flight = native_refresh_flight().lock().await;
        match flight.as_mut() {
            Some(waiters) => {
                waiters.push(sender);
                false
            }
            None => {
                *flight = Some(vec![sender]);
                true
            }
        }
    };
    if starts_refresh {
        let config = config.clone();
        let native_adapter = native_adapter.cloned();
        let worker = tokio::spawn(async move {
            let result = refresh_after_recheck(&config, native_adapter.as_ref()).await;
            let waiters = {
                let mut flight = native_refresh_flight().lock().await;
                flight.take().unwrap_or_default()
            };
            for waiter in waiters {
                let _ = waiter.send(result.clone());
            }
        });
        drop(worker);
    }
    receiver
        .await
        .map_err(|_| CredentialError::NativeRefreshInterrupted)?
}

async fn refresh_after_recheck(
    config: &AppConfig,
    native_adapter: Option<&CodexNativeAdapter>,
) -> NativeRefreshResult {
    let credential = super::codex::resolve_credential(config)
        .map_err(|_| CredentialError::CodexCredentialUnavailable)?
        .ok_or(CredentialError::CodexCredentialUnavailable)?;
    if credential.source() == CodexCredentialSource::ExternalCli || !is_expired(&credential) {
        return Ok(credential);
    }

    let production_adapter;
    let adapter = match native_adapter {
        Some(adapter) => adapter,
        None => {
            production_adapter = CodexNativeAdapter::production()
                .map_err(|_| CredentialError::NativeRefreshFailed)?;
            &production_adapter
        }
    };
    let (cancellation, _handle) = oauth_cancellation();
    adapter
        .refresh(config, &cancellation)
        .await
        .map_err(|_| CredentialError::NativeRefreshFailed)
}
