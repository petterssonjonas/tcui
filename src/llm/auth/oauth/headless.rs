use reqwest::Url;

use super::callback::AuthorizationCode;
use super::response::{parse_authorization_response, parse_authorization_response_without_state};
use super::{CallbackPath, OAuthError, RedirectUri, State};

const MAX_HEADLESS_INPUT_BYTES: usize = 4_096;

pub(crate) enum HeadlessAuthorizationInput {
    RedirectUrl(String),
    Code(String),
}

pub(crate) trait HeadlessInput {
    fn read_authorization_input(&mut self) -> Result<HeadlessAuthorizationInput, OAuthError>;
}

pub(crate) fn complete_headless_authorization(
    input: &mut impl HeadlessInput,
    redirect_uri: &RedirectUri,
    state: &State,
) -> Result<AuthorizationCode, OAuthError> {
    match input.read_authorization_input()? {
        HeadlessAuthorizationInput::RedirectUrl(value) => {
            if value.len() > MAX_HEADLESS_INPUT_BYTES {
                return Err(OAuthError::HeadlessInputTooLarge);
            }
            parse_redirect_url(&value, redirect_uri, state)
        }
        HeadlessAuthorizationInput::Code(value) => {
            if value.len() > MAX_HEADLESS_INPUT_BYTES {
                return Err(OAuthError::HeadlessInputTooLarge);
            }
            AuthorizationCode::parse(value)
        }
    }
}

pub(crate) fn complete_headless_authorization_without_state(
    input: &mut impl HeadlessInput,
    redirect_uri: &RedirectUri,
) -> Result<AuthorizationCode, OAuthError> {
    match input.read_authorization_input()? {
        HeadlessAuthorizationInput::RedirectUrl(value) => {
            if value.len() > MAX_HEADLESS_INPUT_BYTES {
                return Err(OAuthError::HeadlessInputTooLarge);
            }
            parse_redirect_url_without_state(&value, redirect_uri)
        }
        HeadlessAuthorizationInput::Code(value) => {
            if value.len() > MAX_HEADLESS_INPUT_BYTES {
                return Err(OAuthError::HeadlessInputTooLarge);
            }
            AuthorizationCode::parse(value)
        }
    }
}

fn parse_redirect_url(
    value: &str,
    expected_redirect_uri: &RedirectUri,
    state: &State,
) -> Result<AuthorizationCode, OAuthError> {
    let redirect = Url::parse(value).map_err(|_| OAuthError::MalformedCallback)?;
    let expected = expected_redirect_uri.as_url();
    if redirect.scheme() != expected.scheme()
        || redirect.host_str() != expected.host_str()
        || redirect.port_or_known_default() != expected.port_or_known_default()
        || redirect.path() != expected.path()
        || redirect.fragment().is_some()
    {
        return Err(OAuthError::CallbackPath);
    }
    let target = match redirect.query() {
        Some(query) => format!("{}?{query}", redirect.path()),
        None => redirect.path().to_owned(),
    };
    let path = CallbackPath::parse(expected.path())?;
    parse_authorization_response("GET", &target, &path, state)
}

fn parse_redirect_url_without_state(
    value: &str,
    expected_redirect_uri: &RedirectUri,
) -> Result<AuthorizationCode, OAuthError> {
    let redirect = Url::parse(value).map_err(|_| OAuthError::MalformedCallback)?;
    let expected = expected_redirect_uri.as_url();
    if redirect.scheme() != expected.scheme()
        || redirect.host_str() != expected.host_str()
        || redirect.port_or_known_default() != expected.port_or_known_default()
        || redirect.path() != expected.path()
        || redirect.fragment().is_some()
    {
        return Err(OAuthError::CallbackPath);
    }
    let target = match redirect.query() {
        Some(query) => format!("{}?{query}", redirect.path()),
        None => redirect.path().to_owned(),
    };
    let path = CallbackPath::parse(expected.path())?;
    parse_authorization_response_without_state("GET", &target, &path)
}
