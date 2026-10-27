use std::future::Future;
use std::pin::Pin;
use std::time::Duration;

use color_eyre::Result;

use super::{
    AuthCommandArgs, AuthCommandError, AuthCommandRequest, AuthCommandResult, AuthProvider,
};
use crate::llm::auth::oauth::oauth_cancellation;

const SIGNAL_CLEANUP_TIMEOUT: Duration = Duration::from_secs(5);

/// Environment variable whose value, when set, is a file path that the runner
/// creates after the SIGINT handler is armed. This lets integration tests poll
/// for the file's existence to prove the signal handler is registered before
/// sending SIGINT, eliminating the registration race without relying on stdout
/// timing, sleeps, or stderr pipe management.
const SIGNAL_READY_ENV: &str = "TCUI_AUTH_TEST_SIGNAL_READY";

pub(crate) async fn run(arguments: AuthCommandArgs) -> Result<()> {
    let plaintext = arguments.plaintext;
    let vault_password_stdin = arguments.vault_password_stdin;
    let result = match AuthCommandRequest::try_from(arguments) {
        Ok(request) => {
            if let Some(disclosure) = request.disclosure() {
                println!("{disclosure}");
            }
            execute(request, plaintext, vault_password_stdin).await
        }
        Err(error) => Err(error),
    };
    match result {
        Ok(result) => {
            println!("{}", result.message());
            std::process::exit(result.exit_code().as_i32());
        }
        Err(error) => {
            eprintln!("{error}");
            std::process::exit(error.exit_code().as_i32());
        }
    }
}

async fn execute(
    request: AuthCommandRequest,
    plaintext: bool,
    vault_password_stdin: bool,
) -> std::result::Result<AuthCommandResult, AuthCommandError> {
    let provider = request_provider(&request);
    let needs_store = !matches!(&request, AuthCommandRequest::Logout(logout) if logout.external);

    // Eagerly register the SIGINT handler BEFORE creating the flow future.
    //
    // `tokio::signal::ctrl_c()` lazily registers the OS handler on first poll
    // inside the `select!` macro. Between process start and that first poll,
    // SIGINT with the default disposition terminates the process (exit 130)
    // instead of being captured as a cancellation event (exit 11).
    //
    // `signal::unix::signal()` registers the OS handler synchronously during
    // construction, closing the race window entirely: by the time the flow
    // future is created, the handler is already armed.
    #[cfg(unix)]
    let mut sigint = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::interrupt())
        .map_err(|_| AuthCommandError::TransportFailure { provider })?;

    // Create the readiness marker file so integration tests can poll for
    // its existence instead of guessing via stdout timing or stderr pipes.
    if let Some(path) = std::env::var_os(SIGNAL_READY_ENV) {
        let _ = std::fs::write(&path, b"");
    }

    let (cancellation, handle) = oauth_cancellation();
    let flow = async {
        let _session = if needs_store {
            Some(prepare_store(provider, plaintext, vault_password_stdin).await?)
        } else {
            None
        };
        super::flow::execute(request, &cancellation).await
    };
    tokio::pin!(flow);

    #[cfg(unix)]
    {
        tokio::select! {
            biased;
            _ = sigint.recv() => {
                handle.cancel();
                cancelled_after_teardown(provider, flow.as_mut()).await
            }
            result = &mut flow => result,
        }
    }

    #[cfg(not(unix))]
    {
        tokio::select! {
            biased;
            signal = tokio::signal::ctrl_c() => {
                if signal.is_err() {
                    return Err(AuthCommandError::TransportFailure { provider });
                }
                handle.cancel();
                cancelled_after_teardown(provider, flow.as_mut()).await
            }
            result = &mut flow => result,
        }
    }
}

async fn cancelled_after_teardown<F>(
    provider: AuthProvider,
    flow: Pin<&mut F>,
) -> std::result::Result<AuthCommandResult, AuthCommandError>
where
    F: Future<Output = std::result::Result<AuthCommandResult, AuthCommandError>>,
{
    let _ = tokio::time::timeout(SIGNAL_CLEANUP_TIMEOUT, flow).await;
    Err(AuthCommandError::Cancelled { provider })
}

const fn request_provider(request: &AuthCommandRequest) -> AuthProvider {
    match request {
        AuthCommandRequest::Login(login) => login.provider,
        AuthCommandRequest::Logout(logout) => logout.provider,
        AuthCommandRequest::Status(status) => match status.provider {
            Some(provider) => provider,
            None => AuthProvider::Codex,
        },
    }
}
async fn prepare_store(
    provider: AuthProvider,
    plaintext: bool,
    password_stdin: bool,
) -> std::result::Result<std::fs::File, AuthCommandError> {
    use crate::config::key_store::CredentialStorage;
    use crate::config::{AppConfig, KeyStore};

    let unavailable = || AuthCommandError::CredentialStore { provider };
    if password_stdin {
        let password = read_vault_password(provider).await?;
        let password = std::str::from_utf8(&password).map_err(|_| unavailable())?;
        let configured = crate::protection::configured().map_err(|_| unavailable())?;
        if configured {
            crate::protection::unlock(password).map_err(|_| unavailable())?;
        }
        crate::protection::require_app().map_err(|_| unavailable())?;
        let config = AppConfig::default();
        let status = KeyStore::vault_status(&config).map_err(|_| unavailable())?;
        if configured {
            if !status.initialized {
                crate::protection::initialize_password(password).map_err(|_| unavailable())?;
            }
        } else if status.initialized {
            KeyStore::unlock_vault(&config, password).map_err(|_| unavailable())?;
        } else {
            KeyStore::initialize_vault(&config, password).map_err(|_| unavailable())?;
        }
    }
    crate::protection::require_app().map_err(|_| unavailable())?;
    let session = crate::protection::session_guard().map_err(|_| unavailable())?;
    let config = AppConfig::load().map_err(|_| unavailable())?;
    if plaintext {
        KeyStore::set_storage_default(&config, CredentialStorage::Plaintext)
            .map_err(|_| unavailable())?;
        return Ok(session);
    }
    if password_stdin {
        KeyStore::set_storage_default(&config, CredentialStorage::Encrypted)
            .map_err(|_| unavailable())?;
        return Ok(session);
    }
    let status = KeyStore::vault_status(&config).map_err(|_| unavailable())?;
    if status.storage == CredentialStorage::Plaintext || status.unlocked {
        Ok(session)
    } else {
        Err(unavailable())
    }
}

async fn read_vault_password(
    provider: AuthProvider,
) -> std::result::Result<zeroize::Zeroizing<Vec<u8>>, AuthCommandError> {
    use std::io::IsTerminal;
    use tokio::io::AsyncReadExt;

    let unavailable = || AuthCommandError::CredentialStore { provider };
    // Read exactly one line: subsequent stdin belongs to the OAuth callback.
    if std::io::stdin().is_terminal() {
        return Err(unavailable());
    }
    let mut input = tokio::io::stdin();
    let mut password = zeroize::Zeroizing::new(Vec::new());
    loop {
        let byte = input.read_u8().await.map_err(|_| unavailable())?;
        if byte == b'\n' {
            break;
        }
        if password.len() >= 4096 {
            return Err(unavailable());
        }
        password.push(byte);
    }
    if password.last() == Some(&b'\r') {
        password.pop();
    }
    Ok(password)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::key_store::{
        test_support::TestEnv, ApiKeyCredential, ApiKeyCredentialOwnership, ApiKeyCredentialSource,
        Credential, CredentialStorage,
    };
    use crate::config::KeyStore;
    use crate::protection::ProtectionPolicy;

    #[test]
    fn owned_auth_operations_reject_locked_entry_before_configuration_or_credential_changes() {
        use clap::Parser;

        let _guard = crate::test_support::env_lock().lock().expect("env lock");
        let environment = TestEnv::new("auth-entry-gate");
        let config = environment.config();
        config.save().expect("save configuration");
        let credential = Credential::ApiKey(
            ApiKeyCredential::new(
                "OpenRouter OAuth",
                "preserved-plaintext-oauth",
                ApiKeyCredentialOwnership::Tcui,
                ApiKeyCredentialSource::OpenRouterPkce,
            )
            .expect("owned key"),
        );
        KeyStore::upsert_credential_with_storage(
            &config,
            &credential,
            CredentialStorage::Plaintext,
        )
        .expect("plaintext OAuth connection");
        crate::protection::configure(
            ProtectionPolicy {
                app_password: true,
                ..ProtectionPolicy::default()
            },
            "auth-entry-password",
        )
        .expect("configure entry gate");
        crate::protection::lock().expect("locked entry");
        let config_path =
            std::path::PathBuf::from(std::env::var_os("XDG_CONFIG_HOME").expect("isolated config"))
                .join("tcui/config.toml");
        let original_config = std::fs::read(&config_path).expect("saved configuration");
        let original_default = KeyStore::vault_status(&config).expect("status").storage;
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        for operation in ["status", "login", "logout"] {
            let cli = crate::Cli::try_parse_from([
                "tcui",
                "auth",
                "--plaintext",
                operation,
                "openrouter",
            ])
            .expect("auth arguments");
            let Some(crate::Command::Auth(arguments)) = cli.command else {
                panic!("auth request");
            };
            let request = AuthCommandRequest::try_from(arguments).expect("owned request");
            let result = runtime.block_on(execute(request, true, false));
            assert!(matches!(
                result,
                Err(AuthCommandError::CredentialStore {
                    provider: AuthProvider::OpenRouter
                })
            ));
        }
        assert_eq!(
            KeyStore::get_credential(&config, "OpenRouter OAuth").expect("preserved OAuth"),
            Some(credential)
        );
        assert_eq!(
            std::fs::read(config_path).expect("unchanged config"),
            original_config
        );
        assert_eq!(
            KeyStore::vault_status(&config).expect("default").storage,
            original_default
        );
    }

    #[test]
    fn owned_auth_store_keeps_policy_session_guard_until_operation_finishes() {
        let _guard = crate::test_support::env_lock().lock().expect("env lock");
        const CHILD: &str = "TCUI_TEST_AUTH_SESSION_GUARD_CHILD";
        if std::env::var_os(CHILD).is_none() {
            // Unrelated pre-exec children can inherit an exclusive flock briefly.
            // Isolate this guard-lifetime contract without retrying authentication.
            let name = std::any::type_name_of_val(
                &owned_auth_store_keeps_policy_session_guard_until_operation_finishes,
            );
            let name = name
                .strip_prefix(concat!(env!("CARGO_CRATE_NAME"), "::"))
                .expect("qualified test name");
            let output = std::process::Command::new(std::env::current_exe().expect("test binary"))
                .args(["--exact", name, "--test-threads=1"])
                .env(CHILD, "1")
                .output()
                .expect("isolated auth guard probe");
            assert!(
                output.status.success(),
                "isolated auth guard failed:\n{}\n{}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr),
            );
            return;
        }
        let _environment = TestEnv::new("auth-session-guard");
        crate::protection::configure(ProtectionPolicy::default(), "").expect("plaintext policy");
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");
        let session = runtime
            .block_on(prepare_store(AuthProvider::OpenRouter, true, false))
            .expect("owned operation session");
        assert!(crate::protection::configure(ProtectionPolicy::default(), "").is_err());
        drop(session);
        crate::protection::configure(ProtectionPolicy::default(), "")
            .expect("policy change after operation");
    }

    #[tokio::test]
    async fn signal_selected_simultaneously_with_transport_discards_transport_result() {
        // Given
        let provider = AuthProvider::OpenRouter;
        let flow = std::future::ready(Err(AuthCommandError::TransportFailure { provider }));
        tokio::pin!(flow);

        // When
        let result = cancelled_after_teardown(provider, flow.as_mut()).await;

        // Then
        assert!(matches!(
            result,
            Err(AuthCommandError::Cancelled {
                provider: AuthProvider::OpenRouter
            })
        ));
    }
}
