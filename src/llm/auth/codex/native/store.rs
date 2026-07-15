use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use chrono::Utc;
use secrecy::ExposeSecret;
use serde::Deserialize;

use super::error::CodexNativeError;
use super::CodexNativeAdapter;
use crate::config::key_store::{OAuthCredential, OAuthCredentialOwnership, OAuthCredentialSource};
use crate::config::{AppConfig, KeyStore};
use crate::llm::auth::codex::credential::CodexCredential;
use crate::llm::auth::oauth::{
    OAuthCancellation, RefreshToken, RefreshTokenExchange, TokenService, TokenSet,
};

pub(super) fn persist_tokens(
    config: &AppConfig,
    tokens: &TokenSet,
    account_fallback: Option<String>,
) -> Result<CodexCredential, CodexNativeError> {
    let account_id = tokens
        .id_token()
        .map(extract_account_id)
        .transpose()?
        .or(account_fallback)
        .ok_or(CodexNativeError::MissingAccount)?;
    let expires_at = tokens.expires_at().ok_or(CodexNativeError::MissingExpiry)?;
    let credential = OAuthCredential {
        provider: "Codex".to_string(),
        access_token: tokens.access_token().as_str().expose_secret().to_owned(),
        refresh_token: tokens
            .refresh_token()
            .map(|token| token.as_str().expose_secret().to_owned()),
        expires_at,
        account_id: Some(account_id),
        ownership: OAuthCredentialOwnership::Tcui,
        source: OAuthCredentialSource::NativeOAuth,
    };
    KeyStore::upsert_oauth(config, &credential)?;
    Ok(CodexCredential::native(credential))
}

pub(super) async fn refresh(
    adapter: &CodexNativeAdapter,
    config: &AppConfig,
    cancellation: &OAuthCancellation,
) -> Result<CodexCredential, CodexNativeError> {
    let credential =
        KeyStore::get_oauth(config, "Codex")?.ok_or(CodexNativeError::MissingAccount)?;
    let refresh_token = credential
        .refresh_token
        .clone()
        .ok_or(CodexNativeError::MissingRefreshToken)?;
    let refresh_token = RefreshToken::parse(refresh_token)?;
    let request = RefreshTokenExchange::new(adapter.endpoints.client_id.clone(), refresh_token);
    let service = TokenService::hardened(adapter.endpoints.token.clone())?;
    let tokens = service.refresh(&request, cancellation, Utc::now()).await?;
    persist_tokens(config, &tokens, credential.account_id)
}

fn extract_account_id(
    token: &crate::llm::auth::oauth::IdToken,
) -> Result<String, CodexNativeError> {
    let mut parts = token.as_str().expose_secret().split('.');
    let _header = parts.next();
    let payload = parts.next().ok_or(CodexNativeError::IdentityToken)?;
    if parts.next().is_none() || parts.next().is_some() {
        return Err(CodexNativeError::IdentityToken);
    }
    let bytes = URL_SAFE_NO_PAD
        .decode(payload)
        .map_err(|_| CodexNativeError::IdentityToken)?;
    let claims = serde_json::from_slice::<IdTokenClaims>(&bytes)
        .map_err(|_| CodexNativeError::IdentityToken)?;
    claims
        .chatgpt_account_id
        .or_else(|| claims.openai_auth.and_then(|auth| auth.chatgpt_account_id))
        .and_then(non_empty)
        .ok_or(CodexNativeError::MissingEntitlement)
}

#[derive(Deserialize)]
struct IdTokenClaims {
    #[serde(default)]
    chatgpt_account_id: Option<String>,
    #[serde(default, rename = "https://api.openai.com/auth")]
    openai_auth: Option<OpenAiAuthClaims>,
}

#[derive(Deserialize)]
struct OpenAiAuthClaims {
    #[serde(default)]
    chatgpt_account_id: Option<String>,
}

fn non_empty(value: String) -> Option<String> {
    let value = value.trim();
    (!value.is_empty()).then(|| value.to_owned())
}
