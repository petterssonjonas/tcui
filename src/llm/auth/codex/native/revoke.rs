use serde::Serialize;

use super::{CodexNativeAdapter, CodexRevocationFailure};
use crate::config::key_store::{OAuthCredential, OAuthCredentialOwnership, OAuthCredentialSource};
use crate::llm::auth::oauth::{OAuthCancellation, OAuthError, OAuthJsonPost, OAuthJsonService};

pub(super) async fn revoke(
    adapter: &CodexNativeAdapter,
    credential: &OAuthCredential,
    cancellation: &OAuthCancellation,
) -> Result<(), CodexRevocationFailure> {
    if credential.ownership != OAuthCredentialOwnership::Tcui
        || credential.source != OAuthCredentialSource::NativeOAuth
    {
        return Err(CodexRevocationFailure::MalformedResponse);
    }
    let (token, token_type_hint, client_id) = match credential.refresh_token.as_deref() {
        Some(token) if !token.trim().is_empty() => (
            token,
            "refresh_token",
            Some(adapter.endpoints.client_id.as_str()),
        ),
        _ => (credential.access_token.as_str(), "access_token", None),
    };
    let body = serde_json::to_vec(&RevokeRequest {
        token,
        token_type_hint,
        client_id,
    })
    .map_err(|_| CodexRevocationFailure::MalformedResponse)?;
    let service =
        OAuthJsonService::hardened(adapter.revocation_timeout).map_err(map_oauth_error)?;
    service
        .post(
            OAuthJsonPost::new(adapter.endpoints.revocation.clone(), body),
            cancellation,
        )
        .await
        .map(|_| ())
        .map_err(map_oauth_error)
}

#[derive(Serialize)]
struct RevokeRequest<'a> {
    token: &'a str,
    token_type_hint: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    client_id: Option<&'a str>,
}

fn map_oauth_error(error: OAuthError) -> CodexRevocationFailure {
    match error {
        OAuthError::Cancelled => CodexRevocationFailure::Cancelled,
        OAuthError::TokenDeadline => CodexRevocationFailure::TimedOut,
        OAuthError::UnexpectedTokenStatus | OAuthError::TokenServer(_) => {
            CodexRevocationFailure::Rejected
        }
        OAuthError::TokenResponseTooLarge | OAuthError::MalformedTokenResponse => {
            CodexRevocationFailure::MalformedResponse
        }
        OAuthError::TokenTransport => CodexRevocationFailure::Transport,
        _ => CodexRevocationFailure::MalformedResponse,
    }
}
