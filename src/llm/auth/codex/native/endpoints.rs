use reqwest::Url;

use crate::llm::auth::oauth::{AuthorizationEndpoint, ClientId, RedirectUri, Scope, TokenEndpoint};

use super::error::CodexNativeError;

const AUTHORIZATION_ENDPOINT: &str = "https://auth.openai.com/oauth/authorize";
const TOKEN_ENDPOINT: &str = "https://auth.openai.com/oauth/token";
const REVOCATION_ENDPOINT: &str = "https://auth.openai.com/oauth/revoke";
const DEVICE_USER_CODE_ENDPOINT: &str = "https://auth.openai.com/api/accounts/deviceauth/usercode";
const DEVICE_TOKEN_ENDPOINT: &str = "https://auth.openai.com/api/accounts/deviceauth/token";
const DEVICE_REDIRECT_URI: &str = "https://auth.openai.com/deviceauth/callback";
const CLIENT_ID: &str = "app_EMoamEEZ73f0CkXaXp7hrann";
const SCOPES: &str =
    "openid profile email offline_access api.connectors.read api.connectors.invoke";

#[derive(Clone)]
pub(super) struct CodexNativeEndpoints {
    pub(super) authorization: AuthorizationEndpoint,
    pub(super) token: TokenEndpoint,
    pub(super) revocation: Url,
    pub(super) device_user_code: Url,
    pub(super) device_token: Url,
    pub(super) client_id: ClientId,
    pub(super) scopes: Scope,
    pub(super) device_redirect_uri: RedirectUri,
    pub(super) callback_port: Option<u16>,
}

impl CodexNativeEndpoints {
    pub(super) fn production() -> Result<Self, CodexNativeError> {
        Self::from_values(
            AUTHORIZATION_ENDPOINT,
            TOKEN_ENDPOINT,
            REVOCATION_ENDPOINT,
            DEVICE_USER_CODE_ENDPOINT,
            DEVICE_TOKEN_ENDPOINT,
            DEVICE_REDIRECT_URI,
            Some(1455),
        )
    }

    fn from_values(
        authorization: &str,
        token: &str,
        revocation: &str,
        device_user_code: &str,
        device_token: &str,
        device_redirect_uri: &str,
        callback_port: Option<u16>,
    ) -> Result<Self, CodexNativeError> {
        Ok(Self {
            authorization: AuthorizationEndpoint::parse(authorization)
                .map_err(|_| CodexNativeError::Configuration)?,
            token: TokenEndpoint::parse(token).map_err(|_| CodexNativeError::Configuration)?,
            revocation: Url::parse(revocation).map_err(|_| CodexNativeError::Configuration)?,
            device_user_code: Url::parse(device_user_code)
                .map_err(|_| CodexNativeError::Configuration)?,
            device_token: Url::parse(device_token).map_err(|_| CodexNativeError::Configuration)?,
            client_id: ClientId::parse(CLIENT_ID).map_err(|_| CodexNativeError::Configuration)?,
            scopes: Scope::parse(SCOPES).map_err(|_| CodexNativeError::Configuration)?,
            device_redirect_uri: RedirectUri::parse(device_redirect_uri)
                .map_err(|_| CodexNativeError::Configuration)?,
            callback_port,
        })
    }

    #[cfg(any(test, debug_assertions))]
    pub(super) fn fixture(
        authorization: &str,
        token: &str,
        device_user_code: &str,
        device_token: &str,
    ) -> Result<Self, CodexNativeError> {
        Self::from_values(
            authorization,
            token,
            token,
            device_user_code,
            device_token,
            DEVICE_REDIRECT_URI,
            None,
        )
    }

    #[cfg(test)]
    pub(super) fn fixture_with_revocation(
        authorization: &str,
        token: &str,
        device_user_code: &str,
        device_token: &str,
        revocation: &str,
    ) -> Result<Self, CodexNativeError> {
        Self::from_values(
            authorization,
            token,
            revocation,
            device_user_code,
            device_token,
            DEVICE_REDIRECT_URI,
            None,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::{
        AUTHORIZATION_ENDPOINT, CLIENT_ID, CodexNativeEndpoints, DEVICE_REDIRECT_URI,
        DEVICE_TOKEN_ENDPOINT, DEVICE_USER_CODE_ENDPOINT, REVOCATION_ENDPOINT, TOKEN_ENDPOINT,
    };

    #[test]
    fn production_endpoints_are_pinned_despite_test_only_overrides() {
        let production =
            CodexNativeEndpoints::production().expect("production endpoints are valid");
        let fixture = CodexNativeEndpoints::fixture_with_revocation(
            "https://override.invalid/authorize",
            "https://override.invalid/token",
            "https://override.invalid/device/usercode",
            "https://override.invalid/device/token",
            "https://override.invalid/revoke",
        )
        .expect("test endpoints are valid");

        assert_eq!(production.authorization.as_str(), AUTHORIZATION_ENDPOINT);
        assert_eq!(production.token.as_str(), TOKEN_ENDPOINT);
        assert_eq!(production.revocation.as_str(), REVOCATION_ENDPOINT);
        assert_eq!(
            production.device_user_code.as_str(),
            DEVICE_USER_CODE_ENDPOINT
        );
        assert_eq!(production.device_token.as_str(), DEVICE_TOKEN_ENDPOINT);
        assert_eq!(
            production.device_redirect_uri.as_url().as_str(),
            DEVICE_REDIRECT_URI
        );
        assert_eq!(production.client_id.as_str(), CLIENT_ID);
        assert_eq!(production.callback_port, Some(1455));
        assert_ne!(
            production.authorization.as_str(),
            fixture.authorization.as_str()
        );
        assert_ne!(production.token.as_str(), fixture.token.as_str());
        assert_ne!(production.revocation.as_str(), fixture.revocation.as_str());
        assert_ne!(
            production.device_user_code.as_str(),
            fixture.device_user_code.as_str()
        );
        assert_ne!(
            production.device_token.as_str(),
            fixture.device_token.as_str()
        );
        assert_ne!(production.callback_port, fixture.callback_port);
    }
}
