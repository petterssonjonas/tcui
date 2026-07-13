use std::time::Duration;

use crate::llm::auth::oauth::{
    CallbackPath, CallbackTimeout, LoopbackCallback, LoopbackCallbackConfig, OAuthError, State,
    oauth_cancellation,
};

use super::callback_support::{callback_fixture, request_for, send_raw_callback};

#[tokio::test]
async fn loopback_callback_rejects_exact_state_mismatch() -> Result<(), Box<dyn std::error::Error>>
{
    let (callback, _) = callback_fixture().await?;
    let request = request_for(&callback, "code=authorization-code&state=wrong");
    let redirect_uri = callback.redirect_uri().clone();
    let (cancellation, handle) = oauth_cancellation();
    let receiver_cancellation = cancellation.clone();
    let receiver = tokio::spawn(async move { callback.receive(&receiver_cancellation).await });

    let response = send_raw_callback(&redirect_uri, &request).await?;
    handle.cancel();
    let error = receiver.await?.unwrap_err();

    assert!(matches!(error, OAuthError::Cancelled));
    assert!(response.starts_with("HTTP/1.1 400"));
    Ok(())
}

#[tokio::test]
async fn loopback_callback_rejects_duplicate_code_parameter()
-> Result<(), Box<dyn std::error::Error>> {
    let (callback, state) = callback_fixture().await?;
    let request = request_for(&callback, &format!("code=one&code=two&state={state}"));
    let redirect_uri = callback.redirect_uri().clone();
    let (cancellation, handle) = oauth_cancellation();
    let receiver_cancellation = cancellation.clone();
    let receiver = tokio::spawn(async move { callback.receive(&receiver_cancellation).await });

    let response = send_raw_callback(&redirect_uri, &request).await?;
    handle.cancel();
    let error = receiver.await?.unwrap_err();

    assert!(matches!(error, OAuthError::Cancelled));
    assert!(response.starts_with("HTTP/1.1 400"));
    Ok(())
}

#[tokio::test]
async fn loopback_callback_rejects_wrong_method() -> Result<(), Box<dyn std::error::Error>> {
    let (callback, state) = callback_fixture().await?;
    let request = format!(
        "POST /callback?code=authorization-code&state={state} HTTP/1.1\r\nHost: localhost\r\n\r\n"
    );
    let redirect_uri = callback.redirect_uri().clone();
    let (cancellation, handle) = oauth_cancellation();
    let receiver_cancellation = cancellation.clone();
    let receiver = tokio::spawn(async move { callback.receive(&receiver_cancellation).await });

    let response = send_raw_callback(&redirect_uri, &request).await?;
    handle.cancel();
    let error = receiver.await?.unwrap_err();

    assert!(matches!(error, OAuthError::Cancelled));
    assert!(response.starts_with("HTTP/1.1 405"));
    Ok(())
}

#[tokio::test]
async fn loopback_callback_rejects_wrong_path() -> Result<(), Box<dyn std::error::Error>> {
    let (callback, state) = callback_fixture().await?;
    let request = format!(
        "GET /unexpected?code=authorization-code&state={state} HTTP/1.1\r\nHost: localhost\r\n\r\n"
    );
    let redirect_uri = callback.redirect_uri().clone();
    let (cancellation, handle) = oauth_cancellation();
    let receiver_cancellation = cancellation.clone();
    let receiver = tokio::spawn(async move { callback.receive(&receiver_cancellation).await });

    let response = send_raw_callback(&redirect_uri, &request).await?;
    handle.cancel();
    let error = receiver.await?.unwrap_err();

    assert!(matches!(error, OAuthError::Cancelled));
    assert!(response.starts_with("HTTP/1.1 404"));
    Ok(())
}

#[tokio::test]
async fn loopback_callback_rejects_oversized_and_malformed_requests()
-> Result<(), Box<dyn std::error::Error>> {
    let (callback, state) = callback_fixture().await?;
    let oversized = format!(
        "GET /callback?code=authorization-code&state={state} HTTP/1.1\r\nHost: localhost\r\nX-Padding: {}\r\n\r\n",
        "x".repeat(8_192)
    );
    let redirect_uri = callback.redirect_uri().clone();
    let (cancellation, handle) = oauth_cancellation();
    let receiver_cancellation = cancellation.clone();
    let receiver = tokio::spawn(async move { callback.receive(&receiver_cancellation).await });

    let response = send_raw_callback(&redirect_uri, &oversized).await?;
    handle.cancel();
    let error = receiver.await?.unwrap_err();

    assert!(matches!(error, OAuthError::Cancelled));
    assert!(response.starts_with("HTTP/1.1 400"));
    Ok(())
}

#[tokio::test]
async fn loopback_callback_rejects_malformed_request_line() -> Result<(), Box<dyn std::error::Error>>
{
    let (callback, _) = callback_fixture().await?;
    let redirect_uri = callback.redirect_uri().clone();
    let (cancellation, handle) = oauth_cancellation();
    let receiver_cancellation = cancellation.clone();
    let receiver = tokio::spawn(async move { callback.receive(&receiver_cancellation).await });

    let response = send_raw_callback(&redirect_uri, "BROKEN\r\n\r\n").await?;
    handle.cancel();
    let error = receiver.await?.unwrap_err();

    assert!(matches!(error, OAuthError::Cancelled));
    assert!(response.starts_with("HTTP/1.1 400"));
    Ok(())
}

#[tokio::test]
async fn loopback_callback_rejects_request_body() -> Result<(), Box<dyn std::error::Error>> {
    let (callback, state) = callback_fixture().await?;
    let request = format!(
        "GET /callback?code=authorization-code&state={state} HTTP/1.1\r\nHost: localhost\r\nCONTENT-LENGTH: 1\r\n\r\n"
    );
    let redirect_uri = callback.redirect_uri().clone();
    let (cancellation, handle) = oauth_cancellation();
    let receiver_cancellation = cancellation.clone();
    let receiver = tokio::spawn(async move { callback.receive(&receiver_cancellation).await });

    let response = send_raw_callback(&redirect_uri, &request).await?;
    handle.cancel();
    let error = receiver.await?.unwrap_err();

    assert!(matches!(error, OAuthError::Cancelled));
    assert!(response.starts_with("HTTP/1.1 400"));
    Ok(())
}

#[tokio::test]
async fn loopback_callback_times_out_and_honors_cancellation() -> Result<(), OAuthError> {
    let state = State::generate()?;
    let config = LoopbackCallbackConfig::new(
        CallbackPath::parse("/callback")?,
        CallbackTimeout::new(Duration::from_millis(10))?,
    );
    let timed_out = LoopbackCallback::bind(config, state).await?;
    let (cancellation, _) = oauth_cancellation();

    assert!(matches!(
        timed_out.receive(&cancellation).await,
        Err(OAuthError::CallbackTimeout)
    ));

    let cancelled = LoopbackCallback::bind(
        LoopbackCallbackConfig::new(
            CallbackPath::parse("/callback")?,
            CallbackTimeout::new(Duration::from_millis(250))?,
        ),
        State::generate()?,
    )
    .await?;
    let (cancellation, handle) = oauth_cancellation();
    handle.cancel();

    assert!(matches!(
        cancelled.receive(&cancellation).await,
        Err(OAuthError::Cancelled)
    ));
    Ok(())
}
