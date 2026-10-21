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
    /// Explicitly store credentials unencrypted in the application SQLite database.
    #[arg(long, global = true, conflicts_with = "vault_password_stdin")]
    pub(crate) plaintext: bool,
    /// Read one password line from piped stdin to initialize or unlock the encrypted vault.
    #[arg(long, global = true)]
    pub(crate) vault_password_stdin: bool,
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
    /// Use device-code authorization for Codex, or manual-only authorization for OpenRouter.
    #[arg(long)]
    headless: bool,
    /// Explicitly open the authorization URL on this machine (never needed over SSH).
    #[arg(long, conflicts_with = "headless")]
    open_browser: bool,
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
                Ok(Self::Login(AuthLoginRequest {
                    provider,
                    headless: login.headless,
                    open_browser: login.open_browser,
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
