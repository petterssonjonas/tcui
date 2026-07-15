use std::fmt;
use std::time::Duration;

use chrono::{DateTime, TimeDelta, Utc};
use oauth2::{
    basic::BasicTokenType, AccessToken as OAuthAccessToken, ExtraTokenFields,
    RefreshToken as OAuthRefreshToken, StandardTokenResponse, TokenResponse,
};
use secrecy::{ExposeSecret, SecretString};
use serde::{Deserialize, Serialize};

use super::{ClientId, OAuthError};

const DEFAULT_EXPIRY_SKEW: Duration = Duration::from_secs(30);

#[derive(Clone, Deserialize, Serialize)]
pub(crate) struct OpenIdTokenFields {
    #[serde(default)]
    id_token: Option<String>,
}

impl fmt::Debug for OpenIdTokenFields {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("OpenIdTokenFields")
            .field("id_token_present", &self.id_token.is_some())
            .finish()
    }
}

impl ExtraTokenFields for OpenIdTokenFields {}

pub(crate) type OpenIdTokenResponse = StandardTokenResponse<OpenIdTokenFields, BasicTokenType>;

#[derive(Clone)]
pub(crate) struct IdToken(SecretString);

impl IdToken {
    fn parse(value: String) -> Result<Self, OAuthError> {
        if value.is_empty() || value.trim() != value {
            return Err(OAuthError::MalformedTokenResponse);
        }
        Ok(Self(SecretString::from(value)))
    }

    pub(crate) fn as_str(&self) -> &SecretString {
        &self.0
    }
}

impl fmt::Debug for IdToken {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("IdToken(<redacted>)")
    }
}

#[derive(Clone)]
pub(crate) struct RefreshToken(SecretString);

impl RefreshToken {
    pub(crate) fn parse(value: String) -> Result<Self, OAuthError> {
        if value.is_empty() || value.trim() != value {
            return Err(OAuthError::InvalidValue);
        }
        Ok(Self(SecretString::from(value)))
    }

    pub(crate) fn as_str(&self) -> &SecretString {
        &self.0
    }

    pub(super) fn as_oauth(&self) -> OAuthRefreshToken {
        OAuthRefreshToken::new(self.0.expose_secret().to_owned())
    }
}

impl fmt::Debug for RefreshToken {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("RefreshToken(<redacted>)")
    }
}

impl fmt::Display for RefreshToken {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("RefreshToken(<redacted>)")
    }
}

pub(crate) struct RefreshTokenExchange {
    pub(super) client_id: ClientId,
    pub(super) refresh_token: RefreshToken,
}

impl RefreshTokenExchange {
    pub(crate) fn new(client_id: ClientId, refresh_token: RefreshToken) -> Self {
        Self {
            client_id,
            refresh_token,
        }
    }
}

pub(crate) struct AccessToken(SecretString);

impl AccessToken {
    fn from_oauth(value: OAuthAccessToken) -> Result<Self, OAuthError> {
        if value.secret().is_empty() || value.secret().trim() != value.secret() {
            return Err(OAuthError::MalformedTokenResponse);
        }
        Ok(Self(SecretString::from(value.secret().to_owned())))
    }

    pub(crate) fn as_str(&self) -> &SecretString {
        &self.0
    }
}

impl fmt::Debug for AccessToken {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("AccessToken(<redacted>)")
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct ExpirySkew(Duration);

impl ExpirySkew {
    pub(crate) fn new(value: Duration) -> Result<Self, OAuthError> {
        TimeDelta::from_std(value).map_err(|_| OAuthError::ExpiryOverflow)?;
        Ok(Self(value))
    }
}

impl Default for ExpirySkew {
    fn default() -> Self {
        Self(DEFAULT_EXPIRY_SKEW)
    }
}

pub(crate) struct TokenSet {
    access_token: AccessToken,
    id_token: Option<IdToken>,
    refresh_token: Option<RefreshToken>,
    expires_at: Option<DateTime<Utc>>,
    token_type: String,
}

impl TokenSet {
    pub(super) fn from_oauth(
        response: OpenIdTokenResponse,
        now: DateTime<Utc>,
        prior_refresh_token: Option<&RefreshToken>,
    ) -> Result<Self, OAuthError> {
        let access_token = AccessToken::from_oauth(response.access_token().clone())?;
        let id_token = response
            .extra_fields()
            .id_token
            .clone()
            .map(IdToken::parse)
            .transpose()?;
        let token_type = response.token_type().as_ref();
        if token_type.trim().is_empty() {
            return Err(OAuthError::MalformedTokenResponse);
        }
        let refresh_token = match response.refresh_token() {
            Some(value) => Some(RefreshToken::parse(value.secret().to_owned())?),
            None => prior_refresh_token.cloned(),
        };
        let expires_in = response.expires_in().map(|duration| duration.as_secs());
        Ok(Self {
            access_token,
            id_token,
            refresh_token,
            expires_at: calculate_expiry(now, expires_in)?,
            token_type: token_type.to_owned(),
        })
    }

    pub(crate) fn access_token(&self) -> &AccessToken {
        &self.access_token
    }

    pub(crate) fn refresh_token(&self) -> Option<&RefreshToken> {
        self.refresh_token.as_ref()
    }

    pub(crate) fn id_token(&self) -> Option<&IdToken> {
        self.id_token.as_ref()
    }

    pub(crate) fn expires_at(&self) -> Option<DateTime<Utc>> {
        self.expires_at
    }

    pub(crate) fn is_usable_at(&self, now: DateTime<Utc>, skew: ExpirySkew) -> bool {
        let Some(expires_at) = self.expires_at else {
            return true;
        };
        let Ok(skew) = TimeDelta::from_std(skew.0) else {
            return false;
        };
        let Some(boundary) = expires_at.checked_sub_signed(skew) else {
            return false;
        };
        now < boundary
    }
}

impl fmt::Debug for TokenSet {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("TokenSet")
            .field("access_token", &"<redacted>")
            .field("id_token_present", &self.id_token.is_some())
            .field("refresh_token_present", &self.refresh_token.is_some())
            .field("expires_at", &self.expires_at)
            .field("token_type", &self.token_type)
            .finish()
    }
}

pub(crate) fn calculate_expiry(
    now: DateTime<Utc>,
    expires_in: Option<u64>,
) -> Result<Option<DateTime<Utc>>, OAuthError> {
    let Some(seconds) = expires_in else {
        return Ok(None);
    };
    let seconds = i64::try_from(seconds).map_err(|_| OAuthError::ExpiryOverflow)?;
    let duration = TimeDelta::try_seconds(seconds).ok_or(OAuthError::ExpiryOverflow)?;
    now.checked_add_signed(duration)
        .map(Some)
        .ok_or(OAuthError::ExpiryOverflow)
}
