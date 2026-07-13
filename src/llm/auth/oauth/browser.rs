use reqwest::Url;

use super::OAuthError;

pub(crate) trait BrowserLauncher {
    fn open(&self, url: &Url) -> Result<(), OAuthError>;
}

pub(crate) struct SystemBrowser;

impl BrowserLauncher for SystemBrowser {
    fn open(&self, url: &Url) -> Result<(), OAuthError> {
        webbrowser::open(url.as_str()).map_err(|_| OAuthError::BrowserLaunch)
    }
}

pub(crate) fn open_authorization_url(
    browser: &impl BrowserLauncher,
    url: &Url,
) -> Result<(), OAuthError> {
    browser.open(url)
}
