use crate::llm::auth::oauth::{OAuthError, oauth_cancellation};

use super::callback_support::{callback_fixture, request_for, send_raw_callback};

#[tokio::test]
async fn loopback_callback_returns_code_for_exact_path_method_and_state()
-> Result<(), Box<dyn std::error::Error>> {
    let (callback, state) = callback_fixture().await?;
    let request = request_for(&callback, &format!("code=authorization-code&state={state}"));
    let redirect_uri = callback.redirect_uri().clone();
    let (cancellation, _) = oauth_cancellation();
    let receiver_cancellation = cancellation.clone();
    let receiver = tokio::spawn(async move { callback.receive(&receiver_cancellation).await });

    let response = send_raw_callback(&redirect_uri, &request).await?;
    let code = receiver.await??;

    assert_eq!(code.as_str(), "authorization-code");
    assert!(response.starts_with("HTTP/1.1 200"));
    Ok(())
}

#[tokio::test]
async fn loopback_callback_propagates_authorization_server_error()
-> Result<(), Box<dyn std::error::Error>> {
    let (callback, state) = callback_fixture().await?;
    let request = request_for(
        &callback,
        &format!("error=access_denied&error_description=declined&state={state}"),
    );
    let redirect_uri = callback.redirect_uri().clone();
    let (cancellation, _) = oauth_cancellation();
    let receiver_cancellation = cancellation.clone();
    let receiver = tokio::spawn(async move { callback.receive(&receiver_cancellation).await });

    let response = send_raw_callback(&redirect_uri, &request).await?;
    let error = receiver.await?.unwrap_err();

    assert!(matches!(error, OAuthError::AuthorizationDenied));
    assert!(response.starts_with("HTTP/1.1 400"));
    Ok(())
}
