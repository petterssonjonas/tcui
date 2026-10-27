use tokio::io::{AsyncBufReadExt, AsyncReadExt, BufReader};

use crate::llm::auth::oauth::{
    BrowserLauncher, HeadlessAuthorizationInput, HeadlessInput, OAuthCancellation, OAuthError,
    SystemBrowser,
};

pub(super) struct PrintingBrowser {
    label: &'static str,
    open_browser: bool,
}

impl PrintingBrowser {
    pub(super) const fn new(label: &'static str, open_browser: bool) -> Self {
        Self {
            label,
            open_browser,
        }
    }
}

impl BrowserLauncher for PrintingBrowser {
    fn open(&self, url: &reqwest::Url) -> Result<(), OAuthError> {
        println!("{}: {url}", self.label);
        println!("Open this URL in your browser. Paste the final response URL and press Enter, even if localhost cannot load.");
        if self.open_browser {
            SystemBrowser.open(url)?;
        }
        Ok(())
    }
}

/// Reads one bounded authorization response without retaining it in command history.
pub(super) async fn read_headless_input(
    cancellation: &OAuthCancellation,
) -> Result<PastedInput, OAuthError> {
    let mut cancellation = cancellation.clone();
    let stdin = tokio::io::stdin();
    let mut reader = BufReader::new(stdin.take(4097));
    let mut input = zeroize::Zeroizing::new(String::new());

    tokio::select! {
        biased;
        _ = cancellation.cancelled() => Err(OAuthError::Cancelled),
        result = reader.read_line(&mut input) => {
            result.map_err(|_| OAuthError::CallbackIo)?;
            if input.len() > 4096 {
                return Err(OAuthError::HeadlessInputTooLarge);
            }
            if cancellation.is_cancelled() {
                Err(OAuthError::Cancelled)
            } else {
                Ok(parse_headless_input(input.trim().to_owned()))
            }
        }
    }
}

pub(super) async fn read_callback_url(
    cancellation: &OAuthCancellation,
) -> Result<zeroize::Zeroizing<String>, OAuthError> {
    let mut input = read_headless_input(cancellation).await?;
    match input.read_authorization_input()? {
        HeadlessAuthorizationInput::RedirectUrl(value) => Ok(zeroize::Zeroizing::new(value)),
        HeadlessAuthorizationInput::Code(value) if value.is_empty() => {
            // Closed stdin does not disable the optional loopback callback.
            std::future::pending().await
        }
        HeadlessAuthorizationInput::Code(_) => Err(OAuthError::MalformedCallback),
    }
}

fn parse_headless_input(input: String) -> PastedInput {
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
