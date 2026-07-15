use std::fmt;

use oauth2::{
    basic::BasicClient, AuthUrl, ClientId as OAuthClientId, PkceCodeChallenge,
    RedirectUrl as OAuthRedirectUrl, Scope as OAuthScope,
};
use reqwest::Url;

use super::{OAuthError, State};

const RESERVED_AUTHORIZATION_PARAMETERS: [&str; 7] = [
    "client_id",
    "code_challenge",
    "code_challenge_method",
    "redirect_uri",
    "response_type",
    "scope",
    "state",
];

#[derive(Clone)]
pub(crate) struct AuthorizationEndpoint(AuthUrl);

impl AuthorizationEndpoint {
    pub(crate) fn parse(value: &str) -> Result<Self, OAuthError> {
        let url = Url::parse(value).map_err(|_| OAuthError::InvalidUrl)?;
        if url.scheme() != "https" || url.fragment().is_some() {
            return Err(OAuthError::InvalidUrl);
        }
        AuthUrl::new(value.to_owned())
            .map(Self)
            .map_err(|_| OAuthError::InvalidUrl)
    }

    fn has_reserved_parameter(&self) -> bool {
        self.0
            .url()
            .query_pairs()
            .any(|(key, _)| RESERVED_AUTHORIZATION_PARAMETERS.contains(&key.as_ref()))
    }

    #[cfg(test)]
    pub(crate) fn as_str(&self) -> &str {
        self.0.as_str()
    }
}

impl fmt::Debug for AuthorizationEndpoint {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("AuthorizationEndpoint(<redacted>)")
    }
}

#[derive(Clone)]
pub(crate) struct RedirectUri(OAuthRedirectUrl);

impl RedirectUri {
    pub(crate) fn parse(value: &str) -> Result<Self, OAuthError> {
        let url = Url::parse(value).map_err(|_| OAuthError::InvalidUrl)?;
        let loopback_http = url.scheme() == "http"
            && matches!(
                url.host_str(),
                Some("127.0.0.1") | Some("::1") | Some("localhost")
            );
        if (!loopback_http && url.scheme() != "https") || url.fragment().is_some() {
            return Err(OAuthError::InvalidUrl);
        }
        OAuthRedirectUrl::new(value.to_owned())
            .map(Self)
            .map_err(|_| OAuthError::InvalidUrl)
    }

    pub(crate) fn as_url(&self) -> &Url {
        self.0.url()
    }

    pub(super) fn as_oauth(&self) -> OAuthRedirectUrl {
        self.0.clone()
    }
}

impl fmt::Debug for RedirectUri {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("RedirectUri(<redacted>)")
    }
}

#[derive(Clone)]
pub(crate) struct ClientId(OAuthClientId);

impl ClientId {
    pub(crate) fn parse(value: &str) -> Result<Self, OAuthError> {
        if value.trim().is_empty() {
            return Err(OAuthError::InvalidValue);
        }
        Ok(Self(OAuthClientId::new(value.to_owned())))
    }

    pub(super) fn as_oauth(&self) -> OAuthClientId {
        self.0.clone()
    }

    pub(crate) fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for ClientId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("ClientId(<redacted>)")
    }
}

#[derive(Clone)]
pub(crate) struct Scope(OAuthScope);

impl Scope {
    pub(crate) fn parse(value: &str) -> Result<Self, OAuthError> {
        if value.is_empty()
            || !value.bytes().all(|byte| {
                byte == b' ' || byte == b'!' || (b'#'..=b'[').contains(&byte) || byte >= b']'
            })
        {
            return Err(OAuthError::InvalidValue);
        }
        Ok(Self(OAuthScope::new(value.to_owned())))
    }

    fn as_oauth(&self) -> OAuthScope {
        self.0.clone()
    }
}

impl fmt::Debug for Scope {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("Scope(<redacted>)")
    }
}

pub(crate) struct AuthorizationRequest {
    client_id: ClientId,
    redirect_uri: RedirectUri,
    state: State,
    challenge: PkceCodeChallenge,
    scope: Option<Scope>,
    extra_parameters: Vec<(String, String)>,
}

impl AuthorizationRequest {
    pub(crate) fn new(
        client_id: ClientId,
        redirect_uri: RedirectUri,
        state: State,
        challenge: PkceCodeChallenge,
        scope: Option<Scope>,
    ) -> Self {
        Self {
            client_id,
            redirect_uri,
            state,
            challenge,
            scope,
            extra_parameters: Vec::new(),
        }
    }

    pub(crate) fn with_extra_parameter(
        mut self,
        name: &str,
        value: &str,
    ) -> Result<Self, OAuthError> {
        if name.is_empty()
            || value.is_empty()
            || RESERVED_AUTHORIZATION_PARAMETERS.contains(&name)
            || self.extra_parameters.iter().any(|(key, _)| key == name)
        {
            return Err(OAuthError::InvalidValue);
        }
        self.extra_parameters
            .push((name.to_owned(), value.to_owned()));
        Ok(self)
    }

    pub(crate) fn build_url(&self, endpoint: &AuthorizationEndpoint) -> Result<Url, OAuthError> {
        if endpoint.has_reserved_parameter() {
            return Err(OAuthError::ReservedAuthorizationParameter);
        }
        let client = BasicClient::new(self.client_id.as_oauth())
            .set_auth_uri(endpoint.0.clone())
            .set_redirect_uri(self.redirect_uri.as_oauth());
        let request = client
            .authorize_url(|| self.state.as_oauth())
            .set_pkce_challenge(self.challenge.clone());
        let mut request = match &self.scope {
            Some(scope) => request.add_scope(scope.as_oauth()),
            None => request,
        };
        for (name, value) in &self.extra_parameters {
            request = request.add_extra_param(name, value);
        }
        Ok(request.url().0)
    }
}

impl fmt::Debug for AuthorizationRequest {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("AuthorizationRequest(<redacted>)")
    }
}
