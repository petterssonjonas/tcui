use std::fmt;

const SUPPORTED_PROVIDER_IDS: &str = "codex, openrouter";

pub(crate) const CODEX_NATIVE_EXPERIMENTAL_DISCLOSURE: &str = "Experimental native Codex authorization is independent from the Codex CLI and may change or stop working. Continue only if you explicitly selected --native.";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum AuthProvider {
    Codex,
    OpenRouter,
}

impl AuthProvider {
    pub(crate) const fn supports_native_authorization(self) -> bool {
        match self {
            Self::Codex => true,
            Self::OpenRouter => false,
        }
    }

    pub(crate) const fn manages_external_credentials(self) -> bool {
        match self {
            Self::Codex => true,
            Self::OpenRouter => false,
        }
    }
}

impl TryFrom<&str> for AuthProvider {
    type Error = AuthCommandError;

    fn try_from(value: &str) -> Result<Self, Self::Error> {
        match value {
            "codex" => Ok(Self::Codex),
            "openrouter" => Ok(Self::OpenRouter),
            unsupported => Err(AuthCommandError::UnsupportedProvider {
                requested: provider_label(unsupported),
            }),
        }
    }
}

impl fmt::Display for AuthProvider {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Codex => formatter.write_str("Codex"),
            Self::OpenRouter => formatter.write_str("OpenRouter"),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct AuthLoginRequest {
    pub(crate) provider: AuthProvider,
    pub(crate) headless: bool,
    pub(crate) native: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct AuthLogoutRequest {
    pub(crate) provider: AuthProvider,
    pub(crate) external: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct AuthStatusRequest {
    pub(crate) provider: Option<AuthProvider>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum AuthCommandRequest {
    Login(AuthLoginRequest),
    Logout(AuthLogoutRequest),
    Status(AuthStatusRequest),
}

impl AuthCommandRequest {
    pub(crate) fn disclosure(&self) -> Option<&'static str> {
        match self {
            Self::Login(login) => {
                if login.provider == AuthProvider::Codex && login.native {
                    Some(CODEX_NATIVE_EXPERIMENTAL_DISCLOSURE)
                } else {
                    None
                }
            }
            Self::Logout(_) | Self::Status(_) => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(i32)]
pub(crate) enum AuthExitCode {
    Success = 0,
    Unauthenticated = 10,
    DeniedOrExpired = 11,
    Unsupported = 12,
    MissingExternalCli = 13,
    TransportFailure = 14,
}

impl AuthExitCode {
    pub(crate) const fn as_i32(self) -> i32 {
        self as i32
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum AuthCommandResult {
    Login { message: String },
    Logout { message: String },
    Status { message: String },
}

impl AuthCommandResult {
    pub(crate) fn message(&self) -> &str {
        match self {
            Self::Login { message, .. }
            | Self::Logout { message, .. }
            | Self::Status { message, .. } => message,
        }
    }

    pub(crate) const fn exit_code(&self) -> AuthExitCode {
        AuthExitCode::Success
    }
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub(crate) enum AuthCommandError {
    #[error("{provider} is not authenticated.")]
    Unauthenticated { provider: AuthProvider },
    #[error("{provider} authorization was denied or expired.")]
    DeniedOrExpired { provider: AuthProvider },
    #[error("{provider} authorization was cancelled.")]
    Cancelled { provider: AuthProvider },
    #[error("{requested} OAuth is unsupported. Supported providers: {SUPPORTED_PROVIDER_IDS}.")]
    UnsupportedProvider { requested: String },
    #[error("{option} is unsupported for {provider}.")]
    UnsupportedOption {
        provider: AuthProvider,
        option: &'static str,
    },
    #[error("The external {provider} CLI is unavailable.")]
    MissingExternalCli { provider: AuthProvider },
    #[error("A transport failure prevented {provider} authorization.")]
    TransportFailure { provider: AuthProvider },
    #[error("TCUI could not access the local {provider} credential store.")]
    CredentialStore { provider: AuthProvider },
    #[error("No provider authentication is configured.")]
    NoAuthenticationConfigured,
}

impl AuthCommandError {
    pub(crate) const fn exit_code(&self) -> AuthExitCode {
        match self {
            Self::Unauthenticated { .. } | Self::NoAuthenticationConfigured => {
                AuthExitCode::Unauthenticated
            }
            Self::DeniedOrExpired { .. } | Self::Cancelled { .. } => AuthExitCode::DeniedOrExpired,
            Self::UnsupportedProvider { .. } | Self::UnsupportedOption { .. } => {
                AuthExitCode::Unsupported
            }
            Self::MissingExternalCli { .. } => AuthExitCode::MissingExternalCli,
            Self::TransportFailure { .. } | Self::CredentialStore { .. } => {
                AuthExitCode::TransportFailure
            }
        }
    }
}

fn provider_label(provider: &str) -> String {
    let mut characters = provider.chars();
    match characters.next() {
        Some(first) => first.to_uppercase().collect::<String>() + characters.as_str(),
        None => String::new(),
    }
}
