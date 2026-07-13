use reqwest::Url;

use super::callback::{AuthorizationCode, CallbackPath};
use super::{OAuthError, State};

pub(crate) fn parse_authorization_response(
    method: &str,
    target: &str,
    expected_path: &CallbackPath,
    expected_state: &State,
) -> Result<AuthorizationCode, OAuthError> {
    parse_response(method, target, expected_path, Some(expected_state))
}

pub(crate) fn parse_authorization_response_without_state(
    method: &str,
    target: &str,
    expected_path: &CallbackPath,
) -> Result<AuthorizationCode, OAuthError> {
    parse_response(method, target, expected_path, None)
}

fn parse_response(
    method: &str,
    target: &str,
    expected_path: &CallbackPath,
    expected_state: Option<&State>,
) -> Result<AuthorizationCode, OAuthError> {
    if method != "GET" {
        return Err(OAuthError::CallbackMethod);
    }
    if !target.starts_with('/') {
        return Err(OAuthError::MalformedCallback);
    }
    let (path, query) = target
        .split_once('?')
        .map_or((target, ""), |(path, query)| (path, query));
    let url = Url::parse(&format!("http://callback.invalid{path}"))
        .map_err(|_| OAuthError::CallbackEncoding)?;
    if !expected_path.matches(url.path()) {
        return Err(OAuthError::CallbackPath);
    }

    let parameters = CallbackParameters::from_query(query)?;
    if let Some(expected_state) = expected_state {
        let state = parameters.state.ok_or(OAuthError::StateMismatch)?;
        if !expected_state.matches(&state) {
            return Err(OAuthError::StateMismatch);
        }
    }
    match (parameters.code, parameters.error.as_deref()) {
        (Some(_), Some(_)) | (None, None) => Err(OAuthError::MalformedCallback),
        (Some(code), None) => AuthorizationCode::parse(code),
        (None, Some("access_denied")) => Err(OAuthError::AuthorizationDenied),
        (None, Some(_)) => Err(OAuthError::AuthorizationFailed),
    }
}

struct CallbackParameters {
    code: Option<String>,
    state: Option<String>,
    error: Option<String>,
}

impl CallbackParameters {
    fn from_query(query: &str) -> Result<Self, OAuthError> {
        let mut parameters = Self {
            code: None,
            state: None,
            error: None,
        };
        for pair in query.split('&') {
            let (key, value) = pair
                .split_once('=')
                .map_or((pair, ""), |(key, value)| (key, value));
            let key = decode_form_component(key)?;
            let value = decode_form_component(value)?;
            let slot = match key.as_ref() {
                "code" => Some(&mut parameters.code),
                "state" => Some(&mut parameters.state),
                "error" => Some(&mut parameters.error),
                "error_description" => None,
                _ => None,
            };
            if let Some(slot) = slot {
                if slot.replace(value).is_some() {
                    return Err(OAuthError::DuplicateCallbackParameter);
                }
            }
        }
        Ok(parameters)
    }
}

fn decode_form_component(value: &str) -> Result<String, OAuthError> {
    let mut bytes = Vec::with_capacity(value.len());
    let source = value.as_bytes();
    let mut index = 0;
    while let Some(byte) = source.get(index).copied() {
        match byte {
            b'+' => bytes.push(b' '),
            b'%' => {
                let high = source.get(index + 1).copied().and_then(hex_value);
                let low = source.get(index + 2).copied().and_then(hex_value);
                let (Some(high), Some(low)) = (high, low) else {
                    return Err(OAuthError::CallbackEncoding);
                };
                bytes.push((high << 4) | low);
                index += 2;
            }
            _ => bytes.push(byte),
        }
        index += 1;
    }
    String::from_utf8(bytes).map_err(|_| OAuthError::CallbackEncoding)
}

const fn hex_value(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::llm::auth::oauth::{CallbackPath, State};

    #[test]
    fn authorization_response_rejects_malformed_percent_and_utf8() -> Result<(), OAuthError> {
        let path = CallbackPath::parse("/callback")?;
        let state = State::generate()?;

        let malformed =
            parse_authorization_response("GET", "/callback?code=%ZZ&state=ignored", &path, &state);
        let invalid_utf8 =
            parse_authorization_response("GET", "/callback?code=%FF&state=ignored", &path, &state);

        assert!(matches!(malformed, Err(OAuthError::CallbackEncoding)));
        assert!(matches!(invalid_utf8, Err(OAuthError::CallbackEncoding)));
        Ok(())
    }
}
