use std::cell::Cell;
use std::time::Duration;

use reqwest::Url;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

use crate::config::AppConfig;
use crate::llm::auth::oauth::{
    oauth_cancellation, BrowserLauncher, CallbackPath, CallbackTimeout, LoopbackCallbackConfig,
    OAuthError, RedirectUri,
};

use super::fixture::{fixture, response};
use super::lifecycle::TestEnv;

struct RecordingBrowser {
    opened: Cell<bool>,
}

impl BrowserLauncher for RecordingBrowser {
    fn open(&self, url: &Url) -> Result<(), OAuthError> {
        if url.scheme() != "https" {
            return Err(OAuthError::InvalidUrl);
        }
        self.opened.set(true);
        Ok(())
    }
}

#[tokio::test]
async fn browser_loopback_flow_uses_hardened_callback_then_persists_exchange(
) -> Result<(), Box<dyn std::error::Error>> {
    browser_round_trip(false).await
}

#[tokio::test]
async fn pasted_callback_uses_original_pkce_then_persists_exchange(
) -> Result<(), Box<dyn std::error::Error>> {
    browser_round_trip(true).await
}

#[expect(
    clippy::await_holding_lock,
    reason = "The process-wide environment fixture must remain isolated through async cleanup."
)]
async fn browser_round_trip(paste: bool) -> Result<(), Box<dyn std::error::Error>> {
    let _guard = crate::test_support::env_lock()
        .lock()
        .expect("env lock poisoned");
    let _env = TestEnv::new("openrouter-browser-flow");
    let fixture = fixture(
        vec![response("200 OK", r#"{"key":"browser-key"}"#)],
        Duration::from_secs(1),
    )
    .await?;
    let callback_config = LoopbackCallbackConfig::new(
        CallbackPath::parse("/callback")?,
        CallbackTimeout::new(Duration::from_secs(1))?,
    );
    let flow = fixture.adapter.begin_loopback(callback_config).await?;
    let browser = RecordingBrowser {
        opened: Cell::new(false),
    };
    flow.open_browser(&browser)?;
    let redirect = flow.redirect_uri().clone();
    let challenge = flow
        .authorization_url()
        .query_pairs()
        .find(|(key, _)| key == "code_challenge")
        .map(|(_, value)| value.into_owned())
        .ok_or("PKCE challenge missing")?;
    let (cancellation, _) = oauth_cancellation();
    let receiver_cancellation = cancellation.clone();
    let (response_tx, response_rx) = tokio::sync::oneshot::channel();
    let receiver = tokio::spawn(async move {
        flow.receive_code_with_redirect(&receiver_cancellation, async {
            response_rx.await.map_err(|_| OAuthError::Cancelled)
        })
        .await
    });

    // When
    if paste {
        let mut response = redirect.as_url().clone();
        response
            .query_pairs_mut()
            .append_pair("code", "browser-code");
        assert!(response_tx
            .send(zeroize::Zeroizing::new(response.to_string()))
            .is_ok());
    } else {
        send_callback(&redirect, "browser-code").await?;
    }
    let grant = receiver.await??;
    fixture
        .adapter
        .exchange_and_persist(&AppConfig::default(), grant, &cancellation)
        .await?;
    let requests = fixture.finish().await?;

    // Then
    assert!(browser.opened.get());
    assert!(requests[0].contains("\"code\":\"browser-code\""));
    let body: serde_json::Value = serde_json::from_str(
        requests[0]
            .split_once("\r\n\r\n")
            .ok_or("exchange body missing")?
            .1,
    )?;
    let verifier = crate::llm::auth::oauth::PkceVerifier::parse(
        body["code_verifier"]
            .as_str()
            .ok_or("PKCE verifier missing")?,
    )?;
    assert_eq!(verifier.s256_challenge().as_str(), challenge);
    Ok(())
}

async fn send_callback(uri: &RedirectUri, code: &str) -> Result<(), Box<dyn std::error::Error>> {
    let url = uri.as_url();
    let host = url.host_str().ok_or("callback host missing")?;
    let port = url.port_or_known_default().ok_or("callback port missing")?;
    let mut stream = TcpStream::connect((host, port)).await?;
    stream
        .write_all(
            format!(
                "GET {}?code={code} HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n",
                url.path()
            )
            .as_bytes(),
        )
        .await?;
    stream.shutdown().await?;
    let mut response = String::new();
    stream.read_to_string(&mut response).await?;
    if !response.starts_with("HTTP/1.1 200") {
        return Err("callback rejected documented authorization code".into());
    }
    Ok(())
}
