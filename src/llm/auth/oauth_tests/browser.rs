use std::cell::Cell;

use reqwest::Url;

use crate::llm::auth::oauth::{BrowserLauncher, OAuthError, SystemBrowser, open_authorization_url};

struct RecordingBrowser {
    opened: Cell<bool>,
}

impl BrowserLauncher for RecordingBrowser {
    fn open(&self, url: &Url) -> Result<(), OAuthError> {
        if url.scheme() != "https" {
            return Err(OAuthError::InvalidUrl);
        }
        self.opened.set(true);
        Ok(())
    }
}

#[test]
fn browser_opening_uses_injected_boundary() -> Result<(), OAuthError> {
    let browser = RecordingBrowser {
        opened: Cell::new(false),
    };
    let url = Url::parse("https://authorization.example/authorize")
        .map_err(|_| OAuthError::InvalidUrl)?;

    open_authorization_url(&browser, &url)?;

    assert!(browser.opened.get());
    Ok(())
}

#[test]
fn system_browser_is_constructible_without_opening_a_url() {
    let _ = SystemBrowser;
}
