use std::io;
use std::time::Duration;

use chrono::Utc;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use tokio::sync::oneshot;

use crate::llm::auth::oauth::{
    oauth_cancellation, AuthorizationCode, AuthorizationCodeExchange, ClientId, OAuthError,
    PkceVerifier, RedirectUri, TokenEndpoint, TokenRequestTimeout, TokenService,
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
async fn slow_dripping_token_body_cannot_outlive_the_request_deadline(
) -> Result<(), Box<dyn std::error::Error>> {
    let listener = TcpListener::bind("127.0.0.1:0").await?;
    let endpoint = TokenEndpoint::parse(&format!("http://{}", listener.local_addr()?))?;
    let (body_started, body_started_receiver) = oneshot::channel();
    let (release, release_receiver) = oneshot::channel::<()>();
    let server = tokio::spawn(async move {
        let (mut stream, _) = listener.accept().await?;
        let mut request = [0_u8; 1_024];
        let _ = stream.read(&mut request).await?;
        stream
            .write_all(
                b"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: 2\r\n\r\n{",
            )
            .await?;
        body_started
            .send(())
            .map_err(|_| io::Error::new(io::ErrorKind::BrokenPipe, "test receiver dropped"))?;
        let _ = release_receiver.await;
        Ok::<(), io::Error>(())
    });
    let client = reqwest::Client::new();
    let timeout = TokenRequestTimeout::new(Duration::from_secs(1))?;
    let (cancellation, _) = oauth_cancellation();
    let request = tokio::spawn(async move {
        TokenService::with_timeout(&client, endpoint, timeout)
            .exchange(&exchange_request()?, &cancellation, Utc::now())
            .await
    });

    body_started_receiver.await?;
    tokio::time::advance(Duration::from_secs(1)).await;
    tokio::task::yield_now().await;
    let result = request.await?;
    let _ = release.send(());
    server.await??;

    assert!(matches!(result, Err(OAuthError::TokenDeadline)));
    Ok(())
}
