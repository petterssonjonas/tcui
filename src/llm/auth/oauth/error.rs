use std::fmt;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum TokenErrorKind {
    AuthorizationPending,
    SlowDown,
    AccessDenied,
    Expired,
    InvalidGrant,
    Other,
}

impl fmt::Display for TokenErrorKind {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let value = match self {
            Self::AuthorizationPending => "authorization_pending",
            Self::SlowDown => "slow_down",
            Self::AccessDenied => "access_denied",
            Self::Expired => "expired",
            Self::InvalidGrant => "invalid_grant",
            Self::Other => "other",
        };
        formatter.write_str(value)
    }
}

#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub(crate) enum OAuthError {
    #[error("OAuth value is invalid")]
    InvalidValue,
    #[error("OAuth URL is invalid")]
    InvalidUrl,
    #[error("authorization endpoint contains a reserved OAuth query parameter")]
    ReservedAuthorizationParameter,
    #[error("OAuth callback could not be read")]
    CallbackIo,
    #[error("OAuth callback method is invalid")]
    CallbackMethod,
    #[error("OAuth callback path is invalid")]
    CallbackPath,
    #[error("OAuth callback is malformed")]
    MalformedCallback,
    #[error("OAuth callback encoding is invalid")]
    CallbackEncoding,
    #[error("OAuth callback body is not allowed")]
    CallbackBody,
    #[error("OAuth callback header is too large")]
    CallbackHeaderTooLarge,
    #[error("OAuth callback repeats a security-sensitive parameter")]
    DuplicateCallbackParameter,
    #[error("OAuth callback state does not match")]
    StateMismatch,
    #[error("authorization was denied")]
    AuthorizationDenied,
    #[error("authorization server returned an error")]
    AuthorizationFailed,
    #[error("OAuth callback timed out")]
    CallbackTimeout,
    #[error("OAuth callback exceeded its invalid-request limit")]
    CallbackAttemptsExceeded,
    #[error("OAuth operation was cancelled")]
    Cancelled,
    #[error("OAuth authorization URL could not be opened")]
    BrowserLaunch,
    #[error("OAuth token endpoint could not be reached")]
    TokenTransport,
    #[error("OAuth token request timed out")]
    TokenDeadline,
    #[error("OAuth token endpoint did not return HTTP 200 or an OAuth error")]
    UnexpectedTokenStatus,
    #[error("OAuth token response is too large")]
    TokenResponseTooLarge,
    #[error("OAuth token response is malformed")]
    MalformedTokenResponse,
    #[error("OAuth token endpoint rejected the request: {0}")]
    TokenServer(TokenErrorKind),
    #[error("OAuth expiry cannot be represented safely")]
    ExpiryOverflow,
    #[error("device authorization was denied")]
    DeviceDenied,
    #[error("device authorization expired")]
    DeviceExpired,
    #[error("device authorization timed out")]
    DeviceDeadline,
    #[error("device authorization deadline cannot be represented safely")]
    DeviceDeadlineOverflow,
    #[error("device polling interval cannot grow safely")]
    PollIntervalOverflow,
    #[error("headless OAuth input is too large")]
    HeadlessInputTooLarge,
}
