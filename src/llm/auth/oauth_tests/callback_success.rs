use crate::llm::auth::oauth::{oauth_cancellation, OAuthError};

use super::callback_support::{callback_fixture, request_for, send_raw_callback};

#[tokio::test]
async fn loopback_callback_returns_code_for_exact_path_method_and_state(
) -> Result<(), Box<dyn std::error::Error>> {
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
async fn loopback_callback_propagates_authorization_server_error(
) -> Result<(), Box<dyn std::error::Error>> {
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

#[tokio::test]
async fn pasted_callback_completes_without_connecting_to_listener() -> Result<(), OAuthError> {
    let (callback, state) = callback_fixture().await?;
    let value = zeroize::Zeroizing::new(format!(
        "{}?code=pasted-code&state={state}",
        callback.redirect_uri().as_url()
    ));
    let (cancellation, _) = oauth_cancellation();
    let code = callback
        .receive_with_redirect(&cancellation, std::future::ready(Ok(value)))
        .await?;
    assert_eq!(code.as_str(), "pasted-code");
    Ok(())
}

#[tokio::test]
async fn pasted_callback_rejects_wrong_state_and_callback_authority() -> Result<(), OAuthError> {
    for suffix in ["?code=private-code&state=wrong", "?code=private-code"] {
        let (callback, _) = callback_fixture().await?;
        let value =
            zeroize::Zeroizing::new(format!("{}{suffix}", callback.redirect_uri().as_url()));
        let (cancellation, _) = oauth_cancellation();
        let error = callback
            .receive_with_redirect(&cancellation, std::future::ready(Ok(value)))
            .await
            .unwrap_err();
        assert!(matches!(error, OAuthError::StateMismatch));
        assert!(!error.to_string().contains("private-code"));
    }
    let (callback, state) = callback_fixture().await?;
    let mut value = callback.redirect_uri().as_url().clone();
    value.set_username("injected").unwrap();
    value.set_query(Some(&format!("code=private-code&state={state}")));
    let (cancellation, _) = oauth_cancellation();
    let error = callback
        .receive_with_redirect(
            &cancellation,
            std::future::ready(Ok(zeroize::Zeroizing::new(value.to_string()))),
        )
        .await
        .unwrap_err();
    assert!(matches!(error, OAuthError::CallbackPath));
    Ok(())
}

#[tokio::test]
async fn pasted_callback_cancellation_wins_over_ready_response() -> Result<(), OAuthError> {
    let (callback, state) = callback_fixture().await?;
    let value = zeroize::Zeroizing::new(format!(
        "{}?code=pasted-code&state={state}",
        callback.redirect_uri().as_url()
    ));
    let (cancellation, handle) = oauth_cancellation();
    handle.cancel();
    let error = callback
        .receive_with_redirect(&cancellation, std::future::ready(Ok(value)))
        .await
        .unwrap_err();
    assert!(matches!(error, OAuthError::Cancelled));
    Ok(())
}

#[tokio::test(start_paused = true)]
async fn pasted_callback_and_loopback_share_timeout() -> Result<(), OAuthError> {
    let (callback, _) = callback_fixture().await?;
    let (cancellation, _) = oauth_cancellation();
    let error = callback
        .receive_with_redirect(&cancellation, std::future::pending())
        .await
        .unwrap_err();
    assert!(matches!(error, OAuthError::CallbackTimeout));
    Ok(())
}

#[tokio::test]
async fn loopback_still_completes_while_pasted_callback_is_pending(
) -> Result<(), Box<dyn std::error::Error>> {
    let (callback, state) = callback_fixture().await?;
    let request = request_for(&callback, &format!("code=loopback-code&state={state}"));
    let redirect_uri = callback.redirect_uri().clone();
    let (cancellation, _) = oauth_cancellation();
    let receiver = tokio::spawn(async move {
        callback
            .receive_with_redirect(&cancellation, std::future::pending())
            .await
    });
    send_raw_callback(&redirect_uri, &request).await?;
    assert_eq!(receiver.await??.as_str(), "loopback-code");
    Ok(())
}

#[tokio::test]
async fn cancelling_pasted_callback_wait_closes_loopback_listener(
) -> Result<(), Box<dyn std::error::Error>> {
    let (callback, _) = callback_fixture().await?;
    let redirect = callback.redirect_uri().as_url().clone();
    let (cancellation, handle) = oauth_cancellation();
    let (started_tx, started_rx) = tokio::sync::oneshot::channel();
    let task = tokio::spawn(async move {
        callback
            .receive_with_redirect(&cancellation, async {
                let _ = started_tx.send(());
                std::future::pending().await
            })
            .await
    });
    started_rx.await?;
    handle.cancel();
    assert!(matches!(task.await?, Err(OAuthError::Cancelled)));
    assert!(tokio::net::TcpStream::connect((
        redirect.host_str().unwrap(),
        redirect.port_or_known_default().unwrap(),
    ))
    .await
    .is_err());
    Ok(())
}
