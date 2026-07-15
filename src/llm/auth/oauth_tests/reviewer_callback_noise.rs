use std::time::Duration;

use crate::llm::auth::oauth::{
    oauth_cancellation, CallbackPath, CallbackTimeout, LoopbackCallback, LoopbackCallbackConfig,
    State,
};

use super::callback_support::send_raw_callback;

#[tokio::test]
async fn loopback_callback_accepts_valid_response_after_four_invalid_connections(
) -> Result<(), Box<dyn std::error::Error>> {
    let state = State::generate()?;
    let state_value = state.as_str().to_owned();
    let callback = LoopbackCallback::bind(
        LoopbackCallbackConfig::new(
            CallbackPath::parse("/callback")?,
            CallbackTimeout::new(Duration::from_secs(1))?,
        ),
        state,
    )
    .await?;
    let redirect_uri = callback.redirect_uri().clone();
    let (cancellation, _) = oauth_cancellation();
    let receiver_cancellation = cancellation.clone();
    let receiver = tokio::spawn(async move { callback.receive(&receiver_cancellation).await });

    let requests = [
        "POST /callback HTTP/1.1\r\nHost: localhost\r\n\r\n".to_owned(),
        "GET /wrong?code=ignored&state=wrong HTTP/1.1\r\nHost: localhost\r\n\r\n".to_owned(),
        "GET /callback?code=%ZZ&state=wrong HTTP/1.1\r\nHost: localhost\r\n\r\n".to_owned(),
        format!(
            "GET /callback?code=one&code=two&state={state_value} HTTP/1.1\r\nHost: localhost\r\n\r\n"
        ),
    ];
    for request in requests {
        let response = send_raw_callback(&redirect_uri, &request).await?;
        assert!(response.starts_with("HTTP/1.1"));
    }

    let response = send_raw_callback(
        &redirect_uri,
        &format!(
            "GET /callback?code=authorization-code&state={state_value} HTTP/1.1\r\nHost: localhost\r\n\r\n"
        ),
    )
    .await?;
    let code = receiver.await??;

    assert_eq!(code.as_str(), "authorization-code");
    assert!(response.starts_with("HTTP/1.1 200"));
    Ok(())
}
