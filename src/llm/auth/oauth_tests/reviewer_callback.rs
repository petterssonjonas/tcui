use crate::llm::auth::oauth::{
    oauth_cancellation, CallbackPath, CallbackTimeout, LoopbackCallback, LoopbackCallbackConfig,
    OAuthError, State,
};

use super::callback_support::{callback_fixture, request_for, send_raw_callback};

#[tokio::test]
async fn loopback_callback_accepts_valid_response_after_retryable_local_noise(
) -> Result<(), Box<dyn std::error::Error>> {
    let (callback, state) = callback_fixture().await?;
    let redirect_uri = callback.redirect_uri().clone();
    let (cancellation, _) = oauth_cancellation();
    let receiver_cancellation = cancellation.clone();
    let receiver = tokio::spawn(async move { callback.receive(&receiver_cancellation).await });

    let invalid = send_raw_callback(
        &redirect_uri,
        "GET /wrong?code=ignored&state=wrong HTTP/1.1\r\nHost: localhost\r\n\r\n",
    )
    .await?;
    let valid = send_raw_callback(
        &redirect_uri,
        &format!(
            "GET /callback?code=authorization-code&state={state} HTTP/1.1\r\nHost: localhost\r\n\r\n"
        ),
    )
    .await?;
    let code = receiver.await??;

    assert!(invalid.starts_with("HTTP/1.1 404"));
    assert!(valid.starts_with("HTTP/1.1 200"));
    assert_eq!(code.as_str(), "authorization-code");
    Ok(())
}

#[tokio::test]
async fn loopback_callback_bounds_repeated_invalid_connections(
) -> Result<(), Box<dyn std::error::Error>> {
    let state = State::generate()?;
    let callback = LoopbackCallback::bind(
        LoopbackCallbackConfig::new(
            CallbackPath::parse("/callback")?,
            CallbackTimeout::new(std::time::Duration::from_secs(1))?,
        ),
        state,
    )
    .await?;
    let redirect_uri = callback.redirect_uri().clone();
    let (cancellation, _) = oauth_cancellation();
    let receiver_cancellation = cancellation.clone();
    let receiver = tokio::spawn(async move { callback.receive(&receiver_cancellation).await });

    for _ in 0..8 {
        let response = send_raw_callback(
            &redirect_uri,
            "POST /callback HTTP/1.1\r\nHost: localhost\r\n\r\n",
        )
        .await?;
        assert!(response.starts_with("HTTP/1.1 405"));
    }

    assert!(matches!(
        receiver.await?,
        Err(OAuthError::CallbackAttemptsExceeded)
    ));
    Ok(())
}

#[tokio::test]
async fn loopback_callback_rejects_invalid_percent_encoding_then_accepts_valid(
) -> Result<(), Box<dyn std::error::Error>> {
    let (callback, state) = callback_fixture().await?;
    let redirect_uri = callback.redirect_uri().clone();
    let valid_request = request_for(&callback, &format!("code=ok&state={state}"));
    let (cancellation, _) = oauth_cancellation();
    let receiver_cancellation = cancellation.clone();
    let receiver = tokio::spawn(async move { callback.receive(&receiver_cancellation).await });

    let invalid = send_raw_callback(
        &redirect_uri,
        "GET /callback?code=%ZZ&state=invalid HTTP/1.1\r\nHost: localhost\r\n\r\n",
    )
    .await?;
    let valid = send_raw_callback(&redirect_uri, &valid_request).await?;
    let code = receiver.await??;

    assert!(invalid.starts_with("HTTP/1.1 400"));
    assert!(valid.starts_with("HTTP/1.1 200"));
    assert_eq!(code.as_str(), "ok");
    Ok(())
}
