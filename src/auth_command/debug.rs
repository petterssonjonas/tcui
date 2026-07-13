use std::time::Duration;

use reqwest::Url;

use super::{AuthCommandError, AuthProvider};
use crate::llm::auth::{
    codex::CodexNativeAdapter,
    openrouter::{OpenRouterAdapter, OpenRouterTestEndpoints},
};

const OPENROUTER_AUTHORIZATION: &str = "TCUI_AUTH_TEST_OPENROUTER_AUTHORIZATION";
const OPENROUTER_CODE_CREATION: &str = "TCUI_AUTH_TEST_OPENROUTER_CODE_CREATION";
const OPENROUTER_EXCHANGE: &str = "TCUI_AUTH_TEST_OPENROUTER_EXCHANGE";
const OPENROUTER_TIMEOUT_MS: &str = "TCUI_AUTH_TEST_OPENROUTER_TIMEOUT_MS";
const CODEX_AUTHORIZATION: &str = "TCUI_AUTH_TEST_CODEX_AUTHORIZATION";
const CODEX_TOKEN: &str = "TCUI_AUTH_TEST_CODEX_TOKEN";
const CODEX_DEVICE_USER_CODE: &str = "TCUI_AUTH_TEST_CODEX_DEVICE_USER_CODE";
const CODEX_DEVICE_TOKEN: &str = "TCUI_AUTH_TEST_CODEX_DEVICE_TOKEN";

pub(super) fn openrouter_adapter() -> Result<OpenRouterAdapter, AuthCommandError> {
    let Some((authorization, code_creation, exchange)) = values([
        OPENROUTER_AUTHORIZATION,
        OPENROUTER_CODE_CREATION,
        OPENROUTER_EXCHANGE,
    ])?
    else {
        return OpenRouterAdapter::production().map_err(|_| transport(AuthProvider::OpenRouter));
    };
    let timeout = std::env::var(OPENROUTER_TIMEOUT_MS)
        .ok()
        .and_then(|value| value.parse::<u64>().ok())
        .map(Duration::from_millis)
        .unwrap_or(Duration::from_secs(30));
    OpenRouterAdapter::for_test(
        OpenRouterTestEndpoints::new(
            local_url(&authorization, AuthProvider::OpenRouter)?,
            local_url(&code_creation, AuthProvider::OpenRouter)?,
            local_url(&exchange, AuthProvider::OpenRouter)?,
        ),
        timeout,
    )
    .map_err(|_| transport(AuthProvider::OpenRouter))
}

pub(super) fn codex_native_adapter() -> Result<CodexNativeAdapter, AuthCommandError> {
    let Some((authorization, token, device_user_code, device_token)) = values4([
        CODEX_AUTHORIZATION,
        CODEX_TOKEN,
        CODEX_DEVICE_USER_CODE,
        CODEX_DEVICE_TOKEN,
    ])?
    else {
        return CodexNativeAdapter::production().map_err(|_| transport(AuthProvider::Codex));
    };
    test_authorization_url(&authorization, AuthProvider::Codex)?;
    let _ = local_url(&token, AuthProvider::Codex)?;
    let _ = local_url(&device_user_code, AuthProvider::Codex)?;
    let _ = local_url(&device_token, AuthProvider::Codex)?;
    CodexNativeAdapter::fixture(&authorization, &token, &device_user_code, &device_token)
        .map_err(|_| transport(AuthProvider::Codex))
}

fn values(names: [&str; 3]) -> Result<Option<(String, String, String)>, AuthCommandError> {
    match (value(names[0]), value(names[1]), value(names[2])) {
        (None, None, None) => Ok(None),
        (Some(first), Some(second), Some(third)) => Ok(Some((first, second, third))),
        _ => Err(transport(AuthProvider::OpenRouter)),
    }
}

fn values4(names: [&str; 4]) -> Result<Option<(String, String, String, String)>, AuthCommandError> {
    match (
        value(names[0]),
        value(names[1]),
        value(names[2]),
        value(names[3]),
    ) {
        (None, None, None, None) => Ok(None),
        (Some(first), Some(second), Some(third), Some(fourth)) => {
            Ok(Some((first, second, third, fourth)))
        }
        _ => Err(transport(AuthProvider::Codex)),
    }
}

fn value(name: &str) -> Option<String> {
    std::env::var(name)
        .ok()
        .filter(|value| !value.trim().is_empty())
}

fn local_url(value: &str, provider: AuthProvider) -> Result<Url, AuthCommandError> {
    let url = Url::parse(value).map_err(|_| transport(provider))?;
    let local_host = matches!(url.host_str(), Some("127.0.0.1" | "localhost" | "::1"));
    if url.scheme() == "http" && local_host && url.port().is_some() {
        Ok(url)
    } else {
        Err(transport(provider))
    }
}

fn test_authorization_url(value: &str, provider: AuthProvider) -> Result<(), AuthCommandError> {
    let url = Url::parse(value).map_err(|_| transport(provider))?;
    if url.scheme() == "https" && url.fragment().is_none() {
        Ok(())
    } else {
        Err(transport(provider))
    }
}

const fn transport(provider: AuthProvider) -> AuthCommandError {
    AuthCommandError::TransportFailure { provider }
}
