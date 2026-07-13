use std::fmt;

use oauth2::AuthorizationCode as OAuthAuthorizationCode;
use std::time::Duration;

use super::OAuthError;

#[derive(Clone)]
pub(crate) struct CallbackPath(String);

impl CallbackPath {
    pub(crate) fn parse(value: &str) -> Result<Self, OAuthError> {
        if !value.starts_with('/') || value.contains(['?', '#']) {
            return Err(OAuthError::InvalidValue);
        }
        Ok(Self(value.to_owned()))
    }

    pub(super) fn matches(&self, value: &str) -> bool {
        self.0 == value
    }

    pub(super) fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for CallbackPath {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("CallbackPath(<redacted>)")
    }
}

#[derive(Clone, Copy)]
pub(crate) struct CallbackTimeout(Duration);

impl CallbackTimeout {
    pub(crate) fn new(value: Duration) -> Result<Self, OAuthError> {
        if value.is_zero() {
            return Err(OAuthError::InvalidValue);
        }
        Ok(Self(value))
    }

    pub(super) fn duration(self) -> Duration {
        self.0
    }
}

pub(crate) struct LoopbackCallbackConfig {
    pub(super) path: CallbackPath,
    pub(super) timeout: CallbackTimeout,
    pub(super) bind_address: String,
    pub(super) redirect_host: String,
}

impl LoopbackCallbackConfig {
    pub(crate) fn new(path: CallbackPath, timeout: CallbackTimeout) -> Self {
        Self {
            path,
            timeout,
            bind_address: "127.0.0.1:0".to_string(),
            redirect_host: "127.0.0.1".to_string(),
        }
    }

    pub(crate) fn fixed_localhost_port(
        path: CallbackPath,
        timeout: CallbackTimeout,
        port: u16,
    ) -> Self {
        Self {
            path,
            timeout,
            bind_address: format!("127.0.0.1:{port}"),
            redirect_host: "localhost".to_string(),
        }
    }
}

#[derive(Clone)]
pub(crate) struct AuthorizationCode(OAuthAuthorizationCode);

impl AuthorizationCode {
    pub(crate) fn parse(value: String) -> Result<Self, OAuthError> {
        if value.is_empty() || value.trim() != value {
            return Err(OAuthError::MalformedCallback);
        }
        Ok(Self(OAuthAuthorizationCode::new(value)))
    }

    pub(super) fn as_oauth(&self) -> OAuthAuthorizationCode {
        self.0.clone()
    }

    pub(crate) fn as_str(&self) -> &str {
        self.0.secret()
    }
}

impl fmt::Debug for AuthorizationCode {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("AuthorizationCode(<redacted>)")
    }
}
