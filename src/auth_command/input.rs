use tokio::io::{AsyncBufReadExt, BufReader};

use crate::llm::auth::oauth::{
    BrowserLauncher, HeadlessAuthorizationInput, HeadlessInput, OAuthCancellation, OAuthError,
    SystemBrowser,
};

pub(super) struct PrintingBrowser {
    label: &'static str,
}

impl PrintingBrowser {
    pub(super) const fn new(label: &'static str) -> Self {
        Self { label }
    }
}

impl BrowserLauncher for PrintingBrowser {
    fn open(&self, url: &reqwest::Url) -> Result<(), OAuthError> {
        println!("{}: {url}", self.label);
        SystemBrowser.open(url)
    }
}

/// Reads headless authorization input from stdin with a cancellable async
/// architecture.
///
/// The previous implementation used `tokio::task::spawn_blocking(read_stdin)`,
/// which created a blocking task that **survived cancellation**: dropping the
/// `JoinHandle` in the `select!` does not abort the underlying `read_line`
/// call, leaving a blocking-thread-pool thread occupied until the process
/// exits.
///
/// This implementation uses `tokio::io::stdin()` with `AsyncBufReadExt::
/// read_line`. The returned future is a proper async future: when the
/// `select!` drops it on cancellation, the async read is cancelled and the
/// underlying blocking operation's result is discarded. The thread returns
/// to the pool on its next yield point.
///
/// Cancel-safety note: `read_line` is documented as NOT cancel-safe because
/// a partially-read line loses buffered data. This is acceptable here because
/// the function reads exactly one line and the process exits immediately
/// after; there is no second read that could observe corrupted buffer state.
pub(super) async fn read_headless_input(
    cancellation: &OAuthCancellation,
) -> Result<PastedInput, OAuthError> {
    let mut cancellation = cancellation.clone();
    let stdin = tokio::io::stdin();
    let mut reader = BufReader::new(stdin);
    let mut input = String::new();

    tokio::select! {
        biased;
        _ = cancellation.cancelled() => Err(OAuthError::Cancelled),
        result = reader.read_line(&mut input) => {
            result.map_err(|_| OAuthError::CallbackIo)?;
            if cancellation.is_cancelled() {
                Err(OAuthError::Cancelled)
            } else {
                Ok(parse_headless_input(input))
            }
        }
    }
}

fn parse_headless_input(input: String) -> PastedInput {
    let input = input.trim().to_owned();
    let parsed = if input.starts_with("http://") || input.starts_with("https://") {
        HeadlessAuthorizationInput::RedirectUrl(input)
    } else {
        HeadlessAuthorizationInput::Code(input)
    };
    PastedInput(Some(parsed))
}

pub(super) struct PastedInput(Option<HeadlessAuthorizationInput>);

impl HeadlessInput for PastedInput {
    fn read_authorization_input(&mut self) -> Result<HeadlessAuthorizationInput, OAuthError> {
        self.0.take().ok_or(OAuthError::InvalidValue)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::llm::auth::oauth::oauth_cancellation;

    #[tokio::test]
    async fn already_latched_cancellation_wins_over_headless_input() {
        // Given
        let (cancellation, handle) = oauth_cancellation();
        handle.cancel();

        // When
        let result = read_headless_input(&cancellation).await;

        // Then
        assert!(matches!(result, Err(OAuthError::Cancelled)));
    }
}
