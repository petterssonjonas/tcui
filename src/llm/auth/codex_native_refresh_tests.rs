#![expect(
    clippy::await_holding_lock,
    reason = "Tests serialize process-global HOME and XDG fixture paths through async OAuth flows."
)]

use chrono::{Duration, Utc};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

use super::codex::{CodexCredentialSource, CodexNativeAdapter};
use super::codex_test_support::TestEnvironment;
use crate::config::key_store::{OAuthCredential, OAuthCredentialOwnership, OAuthCredentialSource};
use crate::config::{AppConfig, KeyStore};
use crate::llm::auth::oauth::oauth_cancellation;

#[tokio::test]
async fn native_refresh_rotates_tokens_and_preserves_account_identity(
) -> Result<(), Box<dyn std::error::Error>> {
    let _guard = crate::test_support::env_lock()
        .lock()
        .expect("environment lock poisoned");
    let environment = TestEnvironment::new("codex-native-refresh")?;
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
    KeyStore::upsert_oauth(
        &config,
        &OAuthCredential {
            provider: "Codex".to_string(),
            access_token: "old-access".to_string(),
            refresh_token: Some("old-refresh".to_string()),
            expires_at: Utc::now() - Duration::minutes(1),
            account_id: Some("refresh-account".to_string()),
            ownership: OAuthCredentialOwnership::Tcui,
            source: OAuthCredentialSource::NativeOAuth,
        },
    )?;
    let (token_endpoint, server) = refresh_server().await?;
    let adapter = CodexNativeAdapter::fixture(
        "https://authorize.example.test/oauth/authorize",
        &token_endpoint,
        &token_endpoint,
        &token_endpoint,
    )?;
    let (cancellation, _) = oauth_cancellation();

    let credential = adapter.refresh(&config, &cancellation).await?;
    let request = server.await??;

    assert!(request.contains("grant_type=refresh_token"));
    assert_eq!(credential.source(), CodexCredentialSource::TcuiNative);
    assert_eq!(credential.account_id(), Some("refresh-account"));
    let stored = KeyStore::get_oauth(&config, "Codex")?
        .ok_or_else(|| std::io::Error::other("missing rotated credential"))?;
    assert_eq!(stored.access_token, "new-access");
    assert_eq!(stored.refresh_token.as_deref(), Some("new-refresh"));
    Ok(())
}

async fn refresh_server() -> Result<
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
        let body = r#"{"access_token":"new-access","refresh_token":"new-refresh","token_type":"Bearer","expires_in":3600}"#;
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
