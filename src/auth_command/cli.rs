use clap::{Args, Subcommand};

use super::{
    AuthCommandError, AuthCommandRequest, AuthLoginRequest, AuthLogoutRequest, AuthProvider,
    AuthStatusRequest,
};

const AUTH_EXIT_CODE_SUMMARY: &str = "Exit codes: 0 success; 10 unauthenticated; 11 denied or expired; 12 unsupported; 13 external CLI unavailable; 14 transport failure.";

#[derive(Debug, Args)]
#[command(
    about = "Manage provider authentication without starting the interactive TUI",
    after_help = AUTH_EXIT_CODE_SUMMARY
)]
pub(crate) struct AuthCommandArgs {
    #[command(subcommand)]
    operation: AuthOperationArgs,
}

#[derive(Debug, Subcommand)]
enum AuthOperationArgs {
    Login(AuthLoginArgs),
    Logout(AuthLogoutArgs),
    Status(AuthStatusArgs),
}

#[derive(Debug, Args)]
#[command(after_help = "Supported providers: codex, openrouter.")]
struct AuthLoginArgs {
    provider: String,
    #[arg(long)]
    headless: bool,
    #[arg(long)]
    native: bool,
}

#[derive(Debug, Args)]
#[command(after_help = "Supported providers: codex, openrouter.")]
struct AuthLogoutArgs {
    provider: String,
    #[arg(long)]
    external: bool,
}

#[derive(Debug, Args)]
#[command(after_help = "Supported providers: codex, openrouter.")]
struct AuthStatusArgs {
    provider: Option<String>,
}

impl TryFrom<AuthCommandArgs> for AuthCommandRequest {
    type Error = AuthCommandError;

    fn try_from(arguments: AuthCommandArgs) -> Result<Self, Self::Error> {
        match arguments.operation {
            AuthOperationArgs::Login(login) => {
                let provider = AuthProvider::try_from(login.provider.as_str())?;
                if login.native && !provider.supports_native_authorization() {
                    return Err(AuthCommandError::UnsupportedOption {
                        provider,
                        option: "--native",
                    });
                }
                Ok(Self::Login(AuthLoginRequest {
                    provider,
                    headless: login.headless,
                    native: login.native,
                }))
            }
            AuthOperationArgs::Logout(logout) => {
                let provider = AuthProvider::try_from(logout.provider.as_str())?;
                if logout.external && !provider.manages_external_credentials() {
                    return Err(AuthCommandError::UnsupportedOption {
                        provider,
                        option: "--external",
                    });
                }
                Ok(Self::Logout(AuthLogoutRequest {
                    provider,
                    external: logout.external,
                }))
            }
            AuthOperationArgs::Status(status) => Ok(Self::Status(AuthStatusRequest {
                provider: status
                    .provider
                    .as_deref()
                    .map(AuthProvider::try_from)
                    .transpose()?,
            })),
        }
    }
}
