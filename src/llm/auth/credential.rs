use std::fmt;

use secrecy::SecretString;

use super::codex::{CodexCredential, CodexCredentialSource};
use super::CredentialSource;

#[derive(Clone)]
pub(crate) struct Credential {
    provider: String,
    source: CredentialSource,
    kind: CredentialKind,
}

#[derive(Clone)]
enum CredentialKind {
    ApiKey(SecretString),
    CodexOAuth(CodexCredential),
}

impl Credential {
    pub(super) fn api_key(provider: String, source: CredentialSource, key: String) -> Self {
        Self {
            provider,
            source,
            kind: CredentialKind::ApiKey(SecretString::from(key)),
        }
    }

    pub(super) fn codex(credential: CodexCredential) -> Self {
        let source = match credential.source() {
            CodexCredentialSource::ExternalCli => CredentialSource::ExternalCodexCli,
            CodexCredentialSource::TcuiNative => CredentialSource::TcuiNativeOAuth,
        };
        Self {
            provider: "Codex".to_string(),
            source,
            kind: CredentialKind::CodexOAuth(credential),
        }
    }

    pub(crate) fn bearer_token(&self) -> &SecretString {
        match &self.kind {
            CredentialKind::ApiKey(key) => key,
            CredentialKind::CodexOAuth(credential) => credential.access_token(),
        }
    }

    #[cfg_attr(
        not(test),
        expect(dead_code, reason = "Todo 7 displays source-aware credential status.")
    )]
    pub(crate) const fn source(&self) -> CredentialSource {
        self.source
    }

    pub(crate) fn account_id(&self) -> Option<&str> {
        match &self.kind {
            CredentialKind::ApiKey(_) => None,
            CredentialKind::CodexOAuth(credential) => credential.account_id(),
        }
    }

    pub(crate) const fn is_codex_oauth(&self) -> bool {
        matches!(self.kind, CredentialKind::CodexOAuth(_))
    }

    #[cfg(test)]
    pub(crate) fn api_key_for_test(provider: &str, key: &str) -> Self {
        Self::api_key(
            provider.to_owned(),
            CredentialSource::Environment,
            key.to_owned(),
        )
    }
}

impl fmt::Debug for Credential {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("Credential")
            .field("provider", &self.provider)
            .field("source", &self.source)
            .field("token", &"<redacted>")
            .finish()
    }
}

impl fmt::Display for Credential {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "Credential(provider={}, source={:?}, token=<redacted>)",
            self.provider, self.source
        )
    }
}
