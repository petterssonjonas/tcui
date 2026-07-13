use std::time::Duration;

use chrono::Utc;
use secrecy::ExposeSecret;

use crate::llm::auth::oauth::{
    ClientId, DeviceCode, DeviceCodeLifetime, DevicePollingRequest, OAuthError, PollInterval,
    TokenService, oauth_cancellation,
};

use super::token_support::token_sequence_fixture;

fn device_request(
    interval: Duration,
    slow_down_increment: Duration,
    lifetime: Duration,
) -> Result<DevicePollingRequest, OAuthError> {
    DevicePollingRequest::new(
        ClientId::parse("client")?,
        DeviceCode::parse("device-code-secret".to_owned())?,
        PollInterval::new(interval)?,
        PollInterval::new(slow_down_increment)?,
        DeviceCodeLifetime::new(lifetime)?,
    )
}

#[tokio::test]
async fn device_polling_handles_pending_slow_down_then_success()
-> Result<(), Box<dyn std::error::Error>> {
    let (endpoint, server) = token_sequence_fixture(vec![
        (
            "400 Bad Request",
            r#"{"error":"authorization_pending"}"#.to_owned(),
        ),
        ("400 Bad Request", r#"{"error":"slow_down"}"#.to_owned()),
        (
            "200 OK",
            r#"{"access_token":"device-access-secret","token_type":"Bearer"}"#.to_owned(),
        ),
    ])
    .await?;
    let client = reqwest::Client::new();
    let service = TokenService::new(&client, endpoint);
    let (cancellation, _) = oauth_cancellation();

    let token_set = service
        .poll_device(
            &device_request(
                Duration::from_millis(1),
                Duration::from_millis(1),
                Duration::from_millis(250),
            )?,
            &cancellation,
            Utc::now(),
        )
        .await?;
    server.await??;

    assert_eq!(
        token_set.access_token().as_str().expose_secret(),
        "device-access-secret"
    );
    Ok(())
}

#[tokio::test]
async fn device_polling_returns_distinct_denied_and_expired_errors()
-> Result<(), Box<dyn std::error::Error>> {
    let (endpoint, server) = token_sequence_fixture(vec![(
        "400 Bad Request",
        r#"{"error":"access_denied"}"#.to_owned(),
    )])
    .await?;
    let client = reqwest::Client::new();
    let service = TokenService::new(&client, endpoint);
    let (cancellation, _) = oauth_cancellation();
    let denied = service
        .poll_device(
            &device_request(
                Duration::from_millis(1),
                Duration::from_millis(1),
                Duration::from_millis(250),
            )?,
            &cancellation,
            Utc::now(),
        )
        .await;
    server.await??;

    assert!(matches!(denied, Err(OAuthError::DeviceDenied)));

    let (endpoint, server) = token_sequence_fixture(vec![(
        "400 Bad Request",
        r#"{"error":"expired_token"}"#.to_owned(),
    )])
    .await?;
    let service = TokenService::new(&client, endpoint);
    let expired = service
        .poll_device(
            &device_request(
                Duration::from_millis(1),
                Duration::from_millis(1),
                Duration::from_millis(250),
            )?,
            &cancellation,
            Utc::now(),
        )
        .await;
    server.await??;

    assert!(matches!(expired, Err(OAuthError::DeviceExpired)));
    Ok(())
}

#[tokio::test]
async fn device_polling_stops_on_deadline_cancellation_and_interval_overflow()
-> Result<(), OAuthError> {
    let client = reqwest::Client::new();
    let endpoint = crate::llm::auth::oauth::TokenEndpoint::parse("http://127.0.0.1:9")?;
    let service = TokenService::new(&client, endpoint);
    let (cancellation, _) = oauth_cancellation();

    assert!(matches!(
        service
            .poll_device(
                &device_request(
                    Duration::from_millis(1),
                    Duration::from_millis(1),
                    Duration::from_millis(1),
                )?,
                &cancellation,
                Utc::now(),
            )
            .await,
        Err(OAuthError::DeviceDeadline)
    ));

    let (cancellation, handle) = oauth_cancellation();
    handle.cancel();
    assert!(matches!(
        service
            .poll_device(
                &device_request(
                    Duration::from_millis(1),
                    Duration::from_millis(1),
                    Duration::from_millis(250),
                )?,
                &cancellation,
                Utc::now(),
            )
            .await,
        Err(OAuthError::Cancelled)
    ));

    let (endpoint, server) = token_sequence_fixture(vec![(
        "400 Bad Request",
        r#"{"error":"slow_down"}"#.to_owned(),
    )])
    .await
    .map_err(|_| OAuthError::TokenTransport)?;
    let service = TokenService::new(&client, endpoint);
    let (cancellation, _) = oauth_cancellation();
    assert!(matches!(
        service
            .poll_device(
                &device_request(
                    Duration::from_millis(1),
                    Duration::MAX,
                    Duration::from_millis(250),
                )?,
                &cancellation,
                Utc::now(),
            )
            .await,
        Err(OAuthError::PollIntervalOverflow)
    ));
    server
        .await
        .map_err(|_| OAuthError::TokenTransport)?
        .map_err(|_| OAuthError::TokenTransport)?;
    Ok(())
}
