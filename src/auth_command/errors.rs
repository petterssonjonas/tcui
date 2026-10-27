use crate::llm::auth::{
    codex::{CodexCliError, CodexNativeError},
    oauth::{OAuthError, TokenErrorKind},
    openrouter::OpenRouterError,
};

use super::{AuthCommandError, AuthProvider};

pub(super) fn map_codex_cli(error: CodexCliError) -> AuthCommandError {
    match error {
        CodexCliError::MissingCli => AuthCommandError::MissingExternalCli {
            provider: AuthProvider::Codex,
        },
        CodexCliError::Cancelled => AuthCommandError::Cancelled {
            provider: AuthProvider::Codex,
        },
        CodexCliError::PostLoginMissing
        | CodexCliError::PostLoginMalformed
        | CodexCliError::CredentialRead
        | CodexCliError::UnsafeExternalFile => AuthCommandError::Unauthenticated {
            provider: AuthProvider::Codex,
        },
        CodexCliError::Launch
        | CodexCliError::NonzeroExit
        | CodexCliError::TimedOut
        | CodexCliError::Wait
        | CodexCliError::ExternalCredentialStillPresent => AuthCommandError::TransportFailure {
            provider: AuthProvider::Codex,
        },
    }
}

pub(super) fn map_native(error: CodexNativeError) -> AuthCommandError {
    match error {
        CodexNativeError::OAuth(error) => map_oauth(AuthProvider::Codex, error),
        CodexNativeError::MissingEntitlement | CodexNativeError::DeviceDenied => {
            AuthCommandError::DeniedOrExpired {
                provider: AuthProvider::Codex,
            }
        }
        CodexNativeError::Store(_) | CodexNativeError::LocalRemoval => {
            AuthCommandError::CredentialStore {
                provider: AuthProvider::Codex,
            }
        }
        CodexNativeError::Configuration
        | CodexNativeError::MissingAccount
        | CodexNativeError::MissingExpiry
        | CodexNativeError::MissingRefreshToken
        | CodexNativeError::DeviceResponse
        | CodexNativeError::IdentityToken => AuthCommandError::TransportFailure {
            provider: AuthProvider::Codex,
        },
    }
}

pub(super) fn map_openrouter(error: OpenRouterError) -> AuthCommandError {
    match error {
        OpenRouterError::OAuth(OAuthError::UnexpectedTokenStatus) => {
            AuthCommandError::DeniedOrExpired {
                provider: AuthProvider::OpenRouter,
            }
        }
        OpenRouterError::OAuth(error) => map_oauth(AuthProvider::OpenRouter, error),
        OpenRouterError::MalformedResponse => AuthCommandError::DeniedOrExpired {
            provider: AuthProvider::OpenRouter,
        },
        OpenRouterError::Configuration | OpenRouterError::CredentialStore => {
            AuthCommandError::CredentialStore {
                provider: AuthProvider::OpenRouter,
            }
        }
    }
}

pub(super) fn map_oauth(provider: AuthProvider, error: OAuthError) -> AuthCommandError {
    match error {
        OAuthError::AuthorizationDenied
        | OAuthError::DeviceDenied
        | OAuthError::DeviceExpired
        | OAuthError::TokenServer(
            TokenErrorKind::AccessDenied | TokenErrorKind::Expired | TokenErrorKind::InvalidGrant,
        ) => AuthCommandError::DeniedOrExpired { provider },
        OAuthError::Cancelled => AuthCommandError::Cancelled { provider },
        OAuthError::InvalidValue
        | OAuthError::InvalidUrl
        | OAuthError::ReservedAuthorizationParameter
        | OAuthError::CallbackIo
        | OAuthError::CallbackMethod
        | OAuthError::CallbackPath
        | OAuthError::MalformedCallback
        | OAuthError::CallbackEncoding
        | OAuthError::CallbackBody
        | OAuthError::CallbackHeaderTooLarge
        | OAuthError::DuplicateCallbackParameter
        | OAuthError::AuthorizationFailed
        | OAuthError::CallbackTimeout
        | OAuthError::CallbackAttemptsExceeded
        | OAuthError::BrowserLaunch
        | OAuthError::TokenTransport
        | OAuthError::TokenDeadline
        | OAuthError::UnexpectedTokenStatus
        | OAuthError::TokenResponseTooLarge
        | OAuthError::MalformedTokenResponse
        | OAuthError::TokenServer(TokenErrorKind::AuthorizationPending)
        | OAuthError::TokenServer(TokenErrorKind::SlowDown)
        | OAuthError::TokenServer(TokenErrorKind::Other)
        | OAuthError::ExpiryOverflow
        | OAuthError::DeviceDeadline
        | OAuthError::DeviceDeadlineOverflow
        | OAuthError::PollIntervalOverflow
        | OAuthError::HeadlessInputTooLarge
        | OAuthError::StateMismatch => AuthCommandError::TransportFailure { provider },
    }
}
