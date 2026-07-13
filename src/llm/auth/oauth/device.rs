use std::fmt;
use std::time::Duration;

use chrono::{DateTime, Utc};
use secrecy::{ExposeSecret, SecretString};
use tokio::time::{Instant, sleep_until};

use super::deadline::OperationDeadline;
use super::token_values::TokenSet;
use super::{ClientId, OAuthCancellation, OAuthError, TokenErrorKind, TokenService};

#[derive(Clone)]
pub(crate) struct DeviceCode(SecretString);

impl PartialEq for DeviceCode {
    fn eq(&self, other: &Self) -> bool {
        self.0.expose_secret() == other.0.expose_secret()
    }
}

impl Eq for DeviceCode {}

impl DeviceCode {
    pub(crate) fn parse(value: String) -> Result<Self, OAuthError> {
        if value.is_empty() || value.trim() != value {
            return Err(OAuthError::InvalidValue);
        }
        Ok(Self(SecretString::from(value)))
    }

    fn as_str(&self) -> &SecretString {
        &self.0
    }
}

impl fmt::Debug for DeviceCode {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("DeviceCode(<redacted>)")
    }
}

impl fmt::Display for DeviceCode {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("DeviceCode(<redacted>)")
    }
}

#[derive(Clone, Copy)]
pub(crate) struct PollInterval(Duration);

impl PollInterval {
    pub(crate) fn new(value: Duration) -> Result<Self, OAuthError> {
        if value.is_zero() {
            return Err(OAuthError::InvalidValue);
        }
        Ok(Self(value))
    }

    fn checked_add(self, other: Self) -> Result<Self, OAuthError> {
        self.0
            .checked_add(other.0)
            .map(Self)
            .ok_or(OAuthError::PollIntervalOverflow)
    }
}

#[derive(Clone, Copy)]
pub(crate) struct DeviceCodeLifetime(Duration);

impl DeviceCodeLifetime {
    pub(crate) fn new(value: Duration) -> Result<Self, OAuthError> {
        if value.is_zero() {
            return Err(OAuthError::InvalidValue);
        }
        Ok(Self(value))
    }
}

pub(crate) struct DevicePollingRequest {
    client_id: ClientId,
    device_code: DeviceCode,
    interval: PollInterval,
    slow_down_increment: PollInterval,
    deadline: Instant,
}

impl DevicePollingRequest {
    pub(crate) fn new(
        client_id: ClientId,
        device_code: DeviceCode,
        interval: PollInterval,
        slow_down_increment: PollInterval,
        lifetime: DeviceCodeLifetime,
    ) -> Result<Self, OAuthError> {
        let deadline = Instant::now()
            .checked_add(lifetime.0)
            .ok_or(OAuthError::DeviceDeadlineOverflow)?;
        Ok(Self {
            client_id,
            device_code,
            interval,
            slow_down_increment,
            deadline,
        })
    }
}

impl TokenService {
    pub(crate) async fn poll_device(
        &self,
        request: &DevicePollingRequest,
        cancellation: &OAuthCancellation,
        now: DateTime<Utc>,
    ) -> Result<TokenSet, OAuthError> {
        let mut interval = request.interval;
        let deadline = OperationDeadline::new(request.deadline, OAuthError::DeviceDeadline);
        loop {
            let next_poll = Instant::now()
                .checked_add(interval.0)
                .ok_or(OAuthError::PollIntervalOverflow)?;
            let wake_at = next_poll.min(request.deadline);
            deadline.race(cancellation, sleep_until(wake_at)).await?;
            let response = self
                .exchange_device_code(
                    request.client_id.as_str(),
                    request.device_code.as_str().expose_secret(),
                    cancellation,
                )
                .await;
            match response {
                Ok(response) => return TokenSet::from_oauth(response, now, None),
                Err(OAuthError::TokenServer(TokenErrorKind::AuthorizationPending)) => {}
                Err(OAuthError::TokenServer(TokenErrorKind::SlowDown)) => {
                    interval = interval.checked_add(request.slow_down_increment)?;
                }
                Err(OAuthError::TokenServer(TokenErrorKind::AccessDenied)) => {
                    return Err(OAuthError::DeviceDenied);
                }
                Err(OAuthError::TokenServer(TokenErrorKind::Expired)) => {
                    return Err(OAuthError::DeviceExpired);
                }
                Err(OAuthError::TokenDeadline) => return Err(OAuthError::DeviceDeadline),
                Err(error) => return Err(error),
            }
        }
    }
}
