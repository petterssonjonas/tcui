#[cfg_attr(
    not(test),
    expect(
        dead_code,
        unused_imports,
        reason = "Todos 5 and 6 consume this isolated OAuth engine."
    )
)]
pub(crate) mod oauth;

#[cfg_attr(
    not(test),
    expect(
        dead_code,
        reason = "Todo 7 wires the static OpenRouter adapter to the non-TUI auth command."
    )
)]
pub(crate) mod openrouter;

pub(crate) mod codex;

mod credential;
mod policy;
mod reader;
mod resolver;

#[cfg(test)]
mod legacy;

#[cfg(test)]
#[path = "auth/resolver_tests.rs"]
mod resolver_tests;

#[cfg(test)]
#[path = "auth/resolver_refresh_failure_tests.rs"]
mod resolver_refresh_failure_tests;

#[cfg(test)]
#[path = "auth/resolver_singleflight_tests.rs"]
mod resolver_singleflight_tests;

#[cfg(test)]
#[path = "auth/resolver_matrix_tests.rs"]
mod resolver_matrix_tests;

#[cfg(test)]
#[path = "auth/resolver_error_tests.rs"]
mod resolver_error_tests;

#[cfg(test)]
#[path = "auth/secret_hygiene_tests.rs"]
mod secret_hygiene_tests;

pub(crate) use credential::Credential;
pub(crate) use policy::{canonical_provider_name, trusted_provider_endpoint};
pub(crate) use resolver::{resolve_provider_credential, CredentialRequest};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum CredentialSource {
    Environment,
    DotEnv,
    LegacyKeyStore,
    TcuiStoredApiKey,
    TcuiNativeOAuth,
    ExternalCodexCli,
    PassiveExternalToken,
}

#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub(crate) enum CredentialError {
    #[error("the configured provider endpoint is not trusted for credentials")]
    UntrustedEndpoint,
    #[error("the TCUI configuration required for Codex credentials is unavailable")]
    CodexConfiguration,
    #[error("the stored Codex credential is corrupt or has invalid metadata")]
    CodexCredentialUnavailable,
    #[error("the expired TCUI-native Codex credential could not be refreshed")]
    NativeRefreshFailed,
    #[error("the concurrent Codex credential refresh did not complete")]
    NativeRefreshInterrupted,
}

#[cfg(test)]
pub(crate) use reader::read_oauth_token;

#[cfg(test)]
pub(crate) use reader::read_codex_account_id;

#[cfg(test)]
pub(crate) use legacy::read_provider_api_key;

#[cfg(test)]
use crate::storage::Storage;

pub(crate) fn redact_secrets(text: &str) -> String {
    text.split_whitespace()
        .map(|word| {
            if looks_like_secret(word) || looks_like_sensitive_assignment(word) {
                "[redacted]"
            } else {
                word
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

fn looks_like_secret(word: &str) -> bool {
    let trimmed =
        word.trim_matches(|c: char| !c.is_ascii_alphanumeric() && c != '-' && c != '_' && c != '.');
    trimmed.starts_with("Bearer")
        || trimmed.starts_with("sk-")
        || trimmed.starts_with("sk_")
        || trimmed.starts_with("ya29.")
        || trimmed.starts_with("eyJ")
        || trimmed.contains("sk-")
        || trimmed.contains("sk_")
        || trimmed.contains("ya29.")
        || trimmed.contains("eyJ")
        || (trimmed.len() >= 48
            && trimmed
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.'))
}

fn looks_like_sensitive_assignment(word: &str) -> bool {
    let lower = word.to_ascii_lowercase();
    let has_sensitive_key = [
        "api_key",
        "apikey",
        "access_token",
        "authorization",
        "bearer",
        "code",
        "code_verifier",
        "device_code",
        "state",
        "token",
        "secret",
    ]
    .iter()
    .any(|key| lower.contains(key));
    has_sensitive_key
        && (word.contains('=') || word.contains(':') || word.contains('?') || word.contains('&'))
}

#[cfg(test)]
#[path = "auth_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "auth/oauth_baseline_tests.rs"]
mod oauth_baseline_tests;

#[cfg(test)]
#[path = "auth/codex_baseline_tests.rs"]
mod codex_baseline_tests;

#[cfg(test)]
#[path = "auth/codex_test_support.rs"]
mod codex_test_support;

#[cfg(test)]
#[path = "auth/codex_external_tests.rs"]
mod codex_external_tests;

#[cfg(test)]
#[path = "auth/codex_external_safety_tests.rs"]
mod codex_external_safety_tests;

#[cfg(test)]
#[path = "auth/codex_cli_tests.rs"]
mod codex_cli_tests;

#[cfg(test)]
#[path = "auth/codex_native_tests.rs"]
mod codex_native_tests;

#[cfg(test)]
#[path = "auth/codex_native_device_tests.rs"]
mod codex_native_device_tests;

#[cfg(test)]
#[path = "auth/codex_native_refresh_tests.rs"]
mod codex_native_refresh_tests;

#[cfg(test)]
#[path = "auth/codex_native_revoke_tests.rs"]
mod codex_native_revoke_tests;

#[cfg(test)]
#[path = "auth/oauth_tests.rs"]
mod oauth_tests;

#[cfg(test)]
#[path = "auth/openrouter_tests.rs"]
mod openrouter_tests;
