use std::fmt;

use reqwest::Url;

use super::super::oauth::{
    complete_headless_authorization_without_state, open_authorization_url, AuthorizationCode,
    BrowserLauncher, HeadlessInput, LoopbackCallback, OAuthCancellation, PkceVerifier, RedirectUri,
};
use super::OpenRouterError;

pub(crate) struct OpenRouterAuthorization {
    redirect_uri: RedirectUri,
    verifier: PkceVerifier,
    authorization_url: Url,
}

impl OpenRouterAuthorization {
    pub(super) fn new(
        redirect_uri: RedirectUri,
        verifier: PkceVerifier,
        authorization_url: Url,
    ) -> Self {
        Self {
            redirect_uri,
            verifier,
            authorization_url,
        }
    }

    pub(crate) fn authorization_url(&self) -> &Url {
        &self.authorization_url
    }

    pub(super) fn redirect_uri(&self) -> &str {
        self.redirect_uri.as_url().as_str()
    }

    pub(super) fn verifier(&self) -> &PkceVerifier {
        &self.verifier
    }

    pub(crate) fn complete_headless(
        self,
        input: &mut impl HeadlessInput,
    ) -> Result<OpenRouterCodeGrant, OpenRouterError> {
        let code = complete_headless_authorization_without_state(input, &self.redirect_uri)?;
        Ok(self.into_grant(code))
    }

    pub(super) fn into_grant(self, code: AuthorizationCode) -> OpenRouterCodeGrant {
        OpenRouterCodeGrant {
            code,
            verifier: self.verifier,
        }
    }
}

pub(crate) struct OpenRouterLoopbackAuthorization {
    authorization: OpenRouterAuthorization,
    callback: LoopbackCallback,
}

impl OpenRouterLoopbackAuthorization {
    pub(super) fn new(authorization: OpenRouterAuthorization, callback: LoopbackCallback) -> Self {
        Self {
            authorization,
            callback,
        }
    }

    pub(crate) fn authorization_url(&self) -> &Url {
        self.authorization.authorization_url()
    }

    pub(crate) fn redirect_uri(&self) -> &RedirectUri {
        self.callback.redirect_uri()
    }

    pub(crate) fn open_browser(
        &self,
        browser: &impl BrowserLauncher,
    ) -> Result<(), OpenRouterError> {
        Ok(open_authorization_url(browser, self.authorization_url())?)
    }

    pub(crate) async fn receive_code(
        self,
        cancellation: &OAuthCancellation,
    ) -> Result<OpenRouterCodeGrant, OpenRouterError> {
        let code = self.callback.receive(cancellation).await?;
        Ok(self.authorization.into_grant(code))
    }
}

pub(crate) struct OpenRouterCodeGrant {
    code: AuthorizationCode,
    verifier: PkceVerifier,
}

impl OpenRouterCodeGrant {
    pub(crate) fn code(&self) -> &str {
        self.code.as_str()
    }

    pub(super) fn into_parts(self) -> (AuthorizationCode, PkceVerifier) {
        (self.code, self.verifier)
    }
}

impl fmt::Debug for OpenRouterAuthorization {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("OpenRouterAuthorization")
            .field("redirect_uri", &self.redirect_uri)
            .field("verifier", &self.verifier)
            .field("authorization_url", &"<redacted>")
            .finish()
    }
}

impl fmt::Debug for OpenRouterCodeGrant {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("OpenRouterCodeGrant(<redacted>)")
    }
}
