#![expect(
    clippy::await_holding_lock,
    reason = "Tests serialize process-global HOME and XDG fixture paths through async OAuth flows."
)]

use std::sync::Mutex;

use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::sync::oneshot;

use super::codex::{CodexCredentialSource, CodexNativeAdapter};
use super::codex_test_support::TestEnvironment;
use crate::config::key_store::{OAuthCredentialOwnership, OAuthCredentialSource};
use crate::config::{AppConfig, KeyStore};
use crate::llm::auth::oauth::{BrowserLauncher, OAuthError, oauth_cancellation};

#[tokio::test]
async fn native_browser_login_persists_encrypted_account_scoped_credential()
-> Result<(), Box<dyn std::error::Error>> {
    let _guard = crate::test_support::env_lock()
        .lock()
        .expect("environment lock poisoned");
    let environment = TestEnvironment::new("codex-native-browser")?;
    let id_token = format!(
        "header.{}.signature",
        URL_SAFE_NO_PAD
            .encode(r#"{"https://api.openai.com/auth":{"chatgpt_account_id":"native-account"}}"#)
    );
    let (token_endpoint, token_server) = token_server(format!(
        r#"{{"access_token":"native-access","refresh_token":"native-refresh","id_token":"{id_token}","token_type":"Bearer","expires_in":3600}}"#
    ))
    .await?;
    let adapter = CodexNativeAdapter::fixture(
        "https://authorize.example.test/oauth/authorize",
        &token_endpoint,
        &token_endpoint,
        &token_endpoint,
    )?;
    let config = AppConfig {
        key_file: Some(
            environment
                .root
                .join("config/tcui/keys.toml")
                .display()
                .to_string(),
        ),
        ..AppConfig::default()
    };
    let (browser, authorization_url) = RecordingBrowser::new();
    let (cancellation, _) = oauth_cancellation();
    let task = tokio::spawn(async move {
        adapter
            .login_browser(&config, &browser, &cancellation)
            .await
    });

    let authorization_url = authorization_url.await?;
    send_authorization_callback(&authorization_url).await?;
    let credential = task.await??;
    let request = token_server.await??;

    assert!(
        authorization_url
            .query_pairs()
            .any(|(key, value)| key == "codex_cli_simplified_flow" && value == "true")
    );
    assert!(
        authorization_url
            .query_pairs()
            .any(|(key, value)| key == "id_token_add_organizations" && value == "true")
    );
    assert!(request.contains("grant_type=authorization_code"));
    assert_eq!(credential.source(), CodexCredentialSource::TcuiNative);
    assert_eq!(credential.account_id(), Some("native-account"));
    let stored = KeyStore::get_oauth(
        &AppConfig {
            key_file: Some(
                environment
                    .root
                    .join("config/tcui/keys.toml")
                    .display()
                    .to_string(),
            ),
            ..AppConfig::default()
        },
        "Codex",
    )?
    .ok_or_else(|| std::io::Error::other("missing native credential"))?;
    assert_eq!(stored.provider, "Codex");
    assert_eq!(stored.access_token, "native-access");
    assert_eq!(stored.refresh_token.as_deref(), Some("native-refresh"));
    assert_eq!(stored.account_id.as_deref(), Some("native-account"));
    assert_eq!(stored.ownership, OAuthCredentialOwnership::Tcui);
    assert_eq!(stored.source, OAuthCredentialSource::NativeOAuth);
    let raw_store = std::fs::read_to_string(environment.root.join("config/tcui/keys.toml"))?;
    assert!(!raw_store.contains("native-access"));
    assert!(!raw_store.contains("native-refresh"));
    Ok(())
}

struct RecordingBrowser {
    sender: Mutex<Option<oneshot::Sender<reqwest::Url>>>,
}

impl RecordingBrowser {
    fn new() -> (Self, oneshot::Receiver<reqwest::Url>) {
        let (sender, receiver) = oneshot::channel();
        (
            Self {
                sender: Mutex::new(Some(sender)),
            },
            receiver,
        )
    }
}

impl BrowserLauncher for RecordingBrowser {
    fn open(&self, url: &reqwest::Url) -> Result<(), OAuthError> {
        let sender = self
            .sender
            .lock()
            .map_err(|_| OAuthError::BrowserLaunch)?
            .take()
            .ok_or(OAuthError::BrowserLaunch)?;
        sender
            .send(url.clone())
            .map_err(|_| OAuthError::BrowserLaunch)
    }
}

async fn token_server(
    body: String,
) -> Result<
    (
        String,
        tokio::task::JoinHandle<Result<String, std::io::Error>>,
    ),
    std::io::Error,
> {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let endpoint = format!("http://{}", listener.local_addr()?);
    let server = tokio::spawn(async move {
        let (mut socket, _) = listener.accept().await?;
        let mut request = vec![0_u8; 4096];
        let length = socket.read(&mut request).await?;
        let response = format!(
            "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{}",
            body.len(),
            body
        );
        socket.write_all(response.as_bytes()).await?;
        String::from_utf8(request[..length].to_vec())
            .map_err(|_| std::io::Error::other("request was not utf-8"))
    });
    Ok((endpoint, server))
}

async fn send_authorization_callback(url: &reqwest::Url) -> Result<(), Box<dyn std::error::Error>> {
    let redirect = url
        .query_pairs()
        .find(|(key, _)| key == "redirect_uri")
        .map(|(_, value)| reqwest::Url::parse(&value))
        .ok_or_else(|| std::io::Error::other("missing redirect URI"))??;
    let state = url
        .query_pairs()
        .find(|(key, _)| key == "state")
        .map(|(_, value)| value.into_owned())
        .ok_or_else(|| std::io::Error::other("missing state"))?;
    let host = redirect
        .host_str()
        .ok_or_else(|| std::io::Error::other("missing redirect host"))?;
    let port = redirect
        .port_or_known_default()
        .ok_or_else(|| std::io::Error::other("missing redirect port"))?;
    let mut socket = tokio::net::TcpStream::connect(format!("{host}:{port}")).await?;
    let request = format!(
        "GET {}?code=native-code&state={} HTTP/1.1\r\nHost: {host}\r\nConnection: close\r\n\r\n",
        redirect.path(),
        state
    );
    socket.write_all(request.as_bytes()).await?;
    let mut response = Vec::new();
    socket.read_to_end(&mut response).await?;
    if !response.starts_with(b"HTTP/1.1 200") {
        return Err(std::io::Error::other("callback was rejected").into());
    }
    Ok(())
}
