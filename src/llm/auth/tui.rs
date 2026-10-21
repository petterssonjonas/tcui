use std::time::Duration;

use tokio::sync::mpsc::UnboundedSender;

use crate::config::AppConfig;

pub(crate) use super::oauth::{oauth_cancellation, OAuthCancellation, OAuthCancellationHandle};
use super::oauth::{
    BrowserLauncher, CallbackPath, CallbackTimeout, LoopbackCallbackConfig, OAuthError,
};

// Authorization URLs contain short-lived state. Show them only in the login modal,
// never in chat, diagnostics, persisted history, or Debug output.
pub(crate) enum LoginEvent {
    AuthorizationUrl(String),
    Progress(String),
}

impl std::fmt::Debug for LoginEvent {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::AuthorizationUrl(_) => formatter.write_str("AuthorizationUrl(<redacted>)"),
            Self::Progress(message) => formatter.debug_tuple("Progress").field(message).finish(),
        }
    }
}

struct LoginBrowser {
    events: UnboundedSender<LoginEvent>,
}

impl BrowserLauncher for LoginBrowser {
    fn open(&self, url: &reqwest::Url) -> Result<(), OAuthError> {
        self.events
            .send(LoginEvent::AuthorizationUrl(url.as_str().to_owned()))
            .map_err(|_| OAuthError::Cancelled)?;
        let _ = self.events.send(LoginEvent::Progress(
            "Open this URL in your browser. Paste the final response URL here, even if localhost cannot load.".to_owned(),
        ));
        Ok(())
    }
}

/// Runs native authorization and saves only the resulting TCUI-owned credential.
/// Dropping this future closes its callback listener; cancellation also stops exchanges.
pub(crate) async fn login(
    provider: &str,
    config: AppConfig,
    events: UnboundedSender<LoginEvent>,
    cancellation: OAuthCancellation,
    mut responses: tokio::sync::mpsc::Receiver<zeroize::Zeroizing<String>>,
) -> Result<(), String> {
    if cancellation.is_cancelled() {
        return Err("Authorization cancelled.".to_owned());
    }
    let browser = LoginBrowser { events };
    let input = async { responses.recv().await.ok_or(OAuthError::Cancelled) };
    match super::canonical_provider_name(provider).as_str() {
        "Codex" => {
            let adapter =
                crate::auth_command::codex_native_adapter().map_err(|error| error.to_string())?;
            adapter
                .login_browser(&config, &browser, &cancellation, input)
                .await
                .map_err(|error| error.to_string())?;
        }
        "OpenRouter OAuth" => {
            let adapter =
                crate::auth_command::openrouter_adapter().map_err(|error| error.to_string())?;
            let callback = LoopbackCallbackConfig::new(
                CallbackPath::parse("/auth/openrouter").map_err(|error| error.to_string())?,
                CallbackTimeout::new(Duration::from_secs(300))
                    .map_err(|error| error.to_string())?,
            );
            let authorization = adapter
                .begin_loopback(callback)
                .await
                .map_err(|error| error.to_string())?;
            authorization
                .open_browser(&browser)
                .map_err(|error| error.to_string())?;
            let grant = authorization
                .receive_code_with_redirect(&cancellation, input)
                .await
                .map_err(|error| error.to_string())?;
            let _ = browser
                .events
                .send(LoginEvent::Progress("Saving connection…".to_owned()));
            adapter
                .exchange_and_persist(&config, grant, &cancellation)
                .await
                .map_err(|error| error.to_string())?;
        }
        _ => return Err("Choose OpenAI OAuth or OpenRouter OAuth for browser login.".to_owned()),
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn cancelled_login_never_opens_a_browser_or_saves_credentials() {
        let (cancellation, handle) = oauth_cancellation();
        handle.cancel();
        let (events, mut receiver) = tokio::sync::mpsc::unbounded_channel();
        let (_responses, response_receiver) = tokio::sync::mpsc::channel(1);
        let error = login(
            "Codex",
            AppConfig::default(),
            events,
            cancellation,
            response_receiver,
        )
        .await
        .unwrap_err();
        assert!(error.contains("cancelled"));
        assert!(receiver.try_recv().is_err());
    }

    #[test]
    fn authorization_event_debug_never_prints_oauth_state() {
        let event = LoginEvent::AuthorizationUrl("https://example/?state=private-state".to_owned());
        assert!(!format!("{event:?}").contains("private-state"));
    }
}
