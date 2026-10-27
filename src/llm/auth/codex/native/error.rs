#[derive(Debug, thiserror::Error)]
pub(crate) enum CodexNativeError {
    #[error("native Codex OAuth configuration is invalid")]
    Configuration,
    #[error("native Codex OAuth account is missing the Codex entitlement")]
    MissingEntitlement,
    #[error("native Codex OAuth account identifier is missing")]
    MissingAccount,
    #[error("native Codex OAuth response has no usable expiry")]
    MissingExpiry,
    #[error("native Codex OAuth credential has no refresh token")]
    MissingRefreshToken,
    #[error("native Codex OAuth device authorization was denied")]
    DeviceDenied,
    #[error("native Codex OAuth device authorization response is malformed")]
    DeviceResponse,
    #[error("native Codex OAuth identity token is malformed")]
    IdentityToken,
    #[error("native Codex OAuth protocol error")]
    OAuth(#[from] crate::llm::auth::oauth::OAuthError),
    #[error("native Codex OAuth credential store is unavailable")]
    Store(#[from] crate::config::key_store::KeyStoreError),
    #[error("native Codex OAuth credential could not be removed locally")]
    LocalRemoval,
}
