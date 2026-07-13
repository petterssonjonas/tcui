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
    let result = match AuthCommandRequest::try_from(arguments) {
        Ok(request) => {
            if let Some(disclosure) = request.disclosure() {
                println!("{disclosure}");
            }
            execute(request).await
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
) -> std::result::Result<AuthCommandResult, AuthCommandError> {
    let provider = request_provider(&request);

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
    let flow = super::flow::execute(request, &cancellation);
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

#[cfg(test)]
mod tests {
    use super::*;

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
