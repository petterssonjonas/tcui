use std::io;
use std::time::Duration;

use chrono::Utc;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use tokio::sync::oneshot;

use crate::llm::auth::oauth::{
    oauth_cancellation, AuthorizationCode, AuthorizationCodeExchange, ClientId, DeviceCode,
    DeviceCodeLifetime, DevicePollingRequest, OAuthError, PkceVerifier, PollInterval, RedirectUri,
    TokenEndpoint, TokenService,
};

fn exchange_request() -> Result<AuthorizationCodeExchange, OAuthError> {
    Ok(AuthorizationCodeExchange::new(
        ClientId::parse("client")?,
        RedirectUri::parse("http://127.0.0.1:34567/callback")?,
        AuthorizationCode::parse("authorization-code".to_owned())?,
        PkceVerifier::parse("dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk")?,
    ))
}

#[tokio::test(start_paused = true)]
async fn token_body_read_honors_cancellation_without_wall_clock_sleep(
) -> Result<(), Box<dyn std::error::Error>> {
    let listener = TcpListener::bind("127.0.0.1:0").await?;
    let endpoint = TokenEndpoint::parse(&format!("http://{}", listener.local_addr()?))?;
    let (headers_sent, headers_received) = oneshot::channel();
    let (release, release_received) = oneshot::channel::<()>();
    let server = tokio::spawn(async move {
        let (mut stream, _) = listener.accept().await?;
        let mut request = [0_u8; 1_024];
        let _ = stream.read(&mut request).await?;
        stream
            .write_all(
                b"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: 2\r\n\r\n",
            )
            .await?;
        headers_sent
            .send(())
            .map_err(|_| io::Error::new(io::ErrorKind::BrokenPipe, "test receiver dropped"))?;
        let _ = release_received.await;
        Ok::<(), io::Error>(())
    });
    let client = reqwest::Client::new();
    let (cancellation, handle) = oauth_cancellation();
    let exchange = exchange_request()?;
    let request = tokio::spawn(async move {
        TokenService::new(&client, endpoint)
            .exchange(&exchange, &cancellation, Utc::now())
            .await
    });

    headers_received.await?;
    handle.cancel();
    tokio::time::advance(Duration::from_secs(1)).await;
    tokio::task::yield_now().await;
    let result = tokio::time::timeout(Duration::ZERO, request).await;
    let _ = release.send(());
    server.await??;

    assert!(matches!(result, Ok(Ok(Err(OAuthError::Cancelled)))));
    Ok(())
}

#[tokio::test(start_paused = true)]
async fn expired_device_request_does_not_open_a_connection(
) -> Result<(), Box<dyn std::error::Error>> {
    let listener = TcpListener::bind("127.0.0.1:0").await?;
    let endpoint = TokenEndpoint::parse(&format!("http://{}", listener.local_addr()?))?;
    let (accepted, mut accepted_receiver) = oneshot::channel();
    let server = tokio::spawn(async move {
        let _ = listener.accept().await?;
        let _ = accepted.send(());
        Ok::<(), io::Error>(())
    });
    let client = reqwest::Client::new();
    let request = DevicePollingRequest::new(
        ClientId::parse("client")?,
        DeviceCode::parse("device-code".to_owned())?,
        PollInterval::new(Duration::from_secs(1))?,
        PollInterval::new(Duration::from_secs(1))?,
        DeviceCodeLifetime::new(Duration::from_secs(1))?,
    )?;
    let (cancellation, _) = oauth_cancellation();
    let poll = tokio::spawn(async move {
        TokenService::new(&client, endpoint)
            .poll_device(&request, &cancellation, Utc::now())
            .await
    });

    tokio::task::yield_now().await;
    tokio::time::advance(Duration::from_secs(1)).await;
    tokio::task::yield_now().await;
    let result = poll.await?;
    server.abort();

    assert!(matches!(result, Err(OAuthError::DeviceDeadline)));
    assert!(accepted_receiver.try_recv().is_err());
    Ok(())
}

#[tokio::test(start_paused = true)]
async fn nonresponsive_token_endpoint_honors_monotonic_request_deadline(
) -> Result<(), Box<dyn std::error::Error>> {
    let listener = TcpListener::bind("127.0.0.1:0").await?;
    let endpoint = TokenEndpoint::parse(&format!("http://{}", listener.local_addr()?))?;
    let (accepted, accepted_receiver) = oneshot::channel();
    let (release, release_receiver) = oneshot::channel::<()>();
    let server = tokio::spawn(async move {
        let (_stream, _) = listener.accept().await?;
        let _ = accepted.send(());
        let _ = release_receiver.await;
        Ok::<(), io::Error>(())
    });
    let client = reqwest::Client::new();
    let timeout = crate::llm::auth::oauth::TokenRequestTimeout::new(Duration::from_secs(1))?;
    let (cancellation, _) = oauth_cancellation();
    let exchange = exchange_request()?;
    let request = tokio::spawn(async move {
        TokenService::with_timeout(&client, endpoint, timeout)
            .exchange(&exchange, &cancellation, Utc::now())
            .await
    });

    accepted_receiver.await?;
    tokio::time::advance(Duration::from_secs(1)).await;
    tokio::task::yield_now().await;
    let result = request.await?;
    let _ = release.send(());
    server.await??;

    assert!(
        matches!(result, Err(OAuthError::TokenDeadline)),
        "expected token deadline, received {result:?}"
    );
    Ok(())
}

#[tokio::test(start_paused = true)]
async fn device_request_honors_its_monotonic_deadline_while_endpoint_is_silent(
) -> Result<(), Box<dyn std::error::Error>> {
    let listener = TcpListener::bind("127.0.0.1:0").await?;
    let endpoint = TokenEndpoint::parse(&format!("http://{}", listener.local_addr()?))?;
    let (accepted, accepted_receiver) = oneshot::channel();
    let (release, release_receiver) = oneshot::channel::<()>();
    let server = tokio::spawn(async move {
        let (_stream, _) = listener.accept().await?;
        let _ = accepted.send(());
        let _ = release_receiver.await;
        Ok::<(), io::Error>(())
    });
    let client = reqwest::Client::new();
    let request = DevicePollingRequest::new(
        ClientId::parse("client")?,
        DeviceCode::parse("device-code".to_owned())?,
        PollInterval::new(Duration::from_secs(1))?,
        PollInterval::new(Duration::from_secs(1))?,
        DeviceCodeLifetime::new(Duration::from_secs(2))?,
    )?;
    let (cancellation, _) = oauth_cancellation();
    let poll = tokio::spawn(async move {
        TokenService::new(&client, endpoint)
            .poll_device(&request, &cancellation, Utc::now())
            .await
    });

    tokio::task::yield_now().await;
    tokio::time::advance(Duration::from_secs(1)).await;
    accepted_receiver.await?;
    tokio::time::advance(Duration::from_secs(1)).await;
    tokio::task::yield_now().await;
    let result = poll.await?;
    let _ = release.send(());
    server.await??;

    assert!(
        matches!(result, Err(OAuthError::DeviceDeadline)),
        "expected device deadline, received {result:?}"
    );
    Ok(())
}
