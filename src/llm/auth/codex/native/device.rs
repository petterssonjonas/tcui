use std::fmt;
use std::time::Duration;

use serde::{Deserialize, Serialize};

use super::error::CodexNativeError;
use super::store::persist_tokens;
use super::{CodexNativeAdapter, NATIVE_DEVICE_TIMEOUT, NATIVE_HTTP_TIMEOUT};
use crate::config::AppConfig;
use crate::llm::auth::oauth::{
    AuthorizationCode, AuthorizationCodeExchange, OAuthCancellation, OAuthJsonPost,
    OAuthJsonService, PkceVerifier, TokenService,
};

pub(crate) struct CodexDeviceAuthorization {
    device_auth_id: String,
    user_code: String,
    interval: Duration,
}

impl CodexDeviceAuthorization {
    pub(crate) fn user_code(&self) -> &str {
        &self.user_code
    }

    pub(crate) fn verification_url(&self) -> &'static str {
        "https://auth.openai.com/codex/device"
    }
}

impl fmt::Debug for CodexDeviceAuthorization {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("CodexDeviceAuthorization")
            .field("device_auth_id", &"<redacted>")
            .field("user_code", &"<redacted>")
            .field("interval", &self.interval)
            .finish()
    }
}

pub(super) async fn begin(
    adapter: &CodexNativeAdapter,
    cancellation: &OAuthCancellation,
) -> Result<CodexDeviceAuthorization, CodexNativeError> {
    let service = OAuthJsonService::hardened(NATIVE_HTTP_TIMEOUT)?;
    let body = serde_json::to_vec(&DeviceStartRequest {
        client_id: adapter.endpoints.client_id.as_str(),
    })
    .map_err(|_| CodexNativeError::DeviceResponse)?;
    let response = service
        .post(
            OAuthJsonPost::new(adapter.endpoints.device_user_code.clone(), body),
            cancellation,
        )
        .await?;
    let response = serde_json::from_slice::<DeviceStartResponse>(&response)
        .map_err(|_| CodexNativeError::DeviceResponse)?;
    let interval = response.interval.unwrap_or(5);
    if interval == 0
        || response.device_auth_id.trim().is_empty()
        || response.user_code.trim().is_empty()
    {
        return Err(CodexNativeError::DeviceResponse);
    }
    Ok(CodexDeviceAuthorization {
        device_auth_id: response.device_auth_id,
        user_code: response.user_code,
        interval: Duration::from_secs(interval),
    })
}

pub(super) async fn complete(
    adapter: &CodexNativeAdapter,
    config: &AppConfig,
    authorization: CodexDeviceAuthorization,
    cancellation: &OAuthCancellation,
) -> Result<super::super::credential::CodexCredential, CodexNativeError> {
    let service = OAuthJsonService::hardened(NATIVE_HTTP_TIMEOUT)?;
    let deadline = tokio::time::Instant::now()
        .checked_add(NATIVE_DEVICE_TIMEOUT)
        .ok_or(CodexNativeError::DeviceResponse)?;
    let mut interval = authorization.interval;
    loop {
        let body = serde_json::to_vec(&DevicePollRequest {
            device_auth_id: &authorization.device_auth_id,
            user_code: &authorization.user_code,
        })
        .map_err(|_| CodexNativeError::DeviceResponse)?;
        let response = service
            .post(
                OAuthJsonPost::new(adapter.endpoints.device_token.clone(), body),
                cancellation,
            )
            .await?;
        let response = serde_json::from_slice::<DevicePollResponse>(&response)
            .map_err(|_| CodexNativeError::DeviceResponse)?;
        if let (Some(code), Some(verifier)) = (response.authorization_code, response.code_verifier)
        {
            return exchange_device_code(adapter, config, cancellation, code, verifier).await;
        }
        if response.error_code.as_deref() == Some("access_denied")
            && response.error_description.as_deref().is_some_and(|value| {
                value
                    .to_ascii_lowercase()
                    .contains("missing_codex_entitlement")
            })
        {
            return Err(CodexNativeError::MissingEntitlement);
        }
        if response.error_code.as_deref() != Some("authorization_pending") {
            return Err(CodexNativeError::DeviceDenied);
        }
        let wake = tokio::time::Instant::now()
            .checked_add(interval)
            .ok_or(CodexNativeError::DeviceResponse)?
            .min(deadline);
        let mut cancellation = cancellation.clone();
        tokio::select! {
            _ = tokio::time::sleep_until(wake) => {}
            _ = cancellation.cancelled() => return Err(CodexNativeError::OAuth(crate::llm::auth::oauth::OAuthError::Cancelled)),
        }
        if tokio::time::Instant::now() >= deadline {
            return Err(CodexNativeError::DeviceDenied);
        }
        interval = interval
            .checked_add(Duration::from_secs(5))
            .ok_or(CodexNativeError::DeviceResponse)?;
    }
}

async fn exchange_device_code(
    adapter: &CodexNativeAdapter,
    config: &AppConfig,
    cancellation: &OAuthCancellation,
    code: String,
    verifier: String,
) -> Result<super::super::credential::CodexCredential, CodexNativeError> {
    let exchange = AuthorizationCodeExchange::new(
        adapter.endpoints.client_id.clone(),
        adapter.endpoints.device_redirect_uri.clone(),
        AuthorizationCode::parse(code)?,
        PkceVerifier::parse(&verifier)?,
    );
    let service = TokenService::hardened(adapter.endpoints.token.clone())?;
    let tokens = service
        .exchange(&exchange, cancellation, chrono::Utc::now())
        .await?;
    persist_tokens(config, &tokens, None)
}

#[derive(Serialize)]
struct DeviceStartRequest<'a> {
    client_id: &'a str,
}

#[derive(Deserialize)]
struct DeviceStartResponse {
    device_auth_id: String,
    user_code: String,
    #[serde(default)]
    interval: Option<u64>,
}

#[derive(Serialize)]
struct DevicePollRequest<'a> {
    device_auth_id: &'a str,
    user_code: &'a str,
}

#[derive(Deserialize)]
struct DevicePollResponse {
    #[serde(default)]
    authorization_code: Option<String>,
    #[serde(default)]
    code_verifier: Option<String>,
    #[serde(default)]
    error_code: Option<String>,
    #[serde(default)]
    error_description: Option<String>,
}
