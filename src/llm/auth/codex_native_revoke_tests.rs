#![expect(
    clippy::await_holding_lock,
    reason = "Tests serialize process-global HOME and XDG fixture paths through async OAuth flows."
)]

use std::time::Duration;

use chrono::{Duration as ChronoDuration, Utc};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

use super::codex::{CodexNativeAdapter, CodexNativeLogout, CodexRevocationFailure};
use super::codex_test_support::TestEnvironment;
use crate::config::key_store::{OAuthCredential, OAuthCredentialOwnership, OAuthCredentialSource};
use crate::config::{AppConfig, KeyStore};
use crate::llm::auth::oauth::oauth_cancellation;

#[tokio::test]
async fn native_logout_revokes_refresh_token_then_removes_only_tcui_credential()
-> Result<(), Box<dyn std::error::Error>> {
    let _guard = crate::test_support::env_lock()
        .lock()
        .expect("environment lock poisoned");
    let environment = TestEnvironment::new("codex-native-revoke-success")?;
    let config = native_config(&environment);
    store_native_credential(&config)?;
    let external = r#"{"tokens":{"access_token":"external-access"}}"#;
    environment.write_external_auth(external)?;
    let (endpoint, server) = revoke_server("200 OK", "{}").await?;
    let adapter = fixture(&endpoint, Duration::from_secs(1))?;
    let (cancellation, _) = oauth_cancellation();

    let outcome = adapter.logout(&config, &cancellation).await?;
    let request = server.await??;

    assert_eq!(outcome, CodexNativeLogout::Revoked);
    assert!(request.contains("\"token_type_hint\":\"refresh_token\""));
    assert!(request.contains("\"client_id\":\"app_EMoamEEZ73f0CkXaXp7hrann\""));
    assert!(!request.contains("native-access"));
    assert!(KeyStore::get_oauth(&config, "Codex")?.is_none());
    assert_eq!(std::fs::read_to_string(environment.auth_path())?, external);
    Ok(())
}

#[tokio::test]
async fn native_logout_removes_local_credential_after_revoke_error_without_touching_external()
-> Result<(), Box<dyn std::error::Error>> {
    let _guard = crate::test_support::env_lock()
        .lock()
        .expect("environment lock poisoned");
    let environment = TestEnvironment::new("codex-native-revoke-error")?;
    let config = native_config(&environment);
    store_native_credential(&config)?;
    let external = r#"{"tokens":{"access_token":"external-access"}}"#;
    environment.write_external_auth(external)?;
    let (endpoint, server) = revoke_server("500 Internal Server Error", "not-json").await?;
    let adapter = fixture(&endpoint, Duration::from_secs(1))?;
    let (cancellation, _) = oauth_cancellation();

    let outcome = adapter.logout(&config, &cancellation).await?;
    let _request = server.await??;

    assert_eq!(
        outcome,
        CodexNativeLogout::RevocationFailed(CodexRevocationFailure::Rejected)
    );
    assert!(KeyStore::get_oauth(&config, "Codex")?.is_none());
    assert_eq!(std::fs::read_to_string(environment.auth_path())?, external);
    Ok(())
}

#[tokio::test]
async fn native_logout_removes_local_credential_after_revoke_timeout()
-> Result<(), Box<dyn std::error::Error>> {
    let _guard = crate::test_support::env_lock()
        .lock()
        .expect("environment lock poisoned");
    let environment = TestEnvironment::new("codex-native-revoke-timeout")?;
    let config = native_config(&environment);
    store_native_credential(&config)?;
    let (endpoint, server) = delayed_revoke_server().await?;
    let adapter = fixture(&endpoint, Duration::from_millis(50))?;
    let (cancellation, _) = oauth_cancellation();

    let outcome = adapter.logout(&config, &cancellation).await?;
    server.abort();

    assert_eq!(
        outcome,
        CodexNativeLogout::RevocationFailed(CodexRevocationFailure::TimedOut)
    );
    assert!(KeyStore::get_oauth(&config, "Codex")?.is_none());
    Ok(())
}

#[tokio::test]
async fn native_logout_removes_local_credential_after_revoke_transport_failure()
-> Result<(), Box<dyn std::error::Error>> {
    let _guard = crate::test_support::env_lock()
        .lock()
        .expect("environment lock poisoned");
    let environment = TestEnvironment::new("codex-native-revoke-transport")?;
    let config = native_config(&environment);
    store_native_credential(&config)?;
    let listener = std::net::TcpListener::bind("127.0.0.1:0")?;
    let endpoint = format!("http://{}", listener.local_addr()?);
    drop(listener);
    let adapter = fixture(&endpoint, Duration::from_secs(1))?;
    let (cancellation, _) = oauth_cancellation();

    let outcome = adapter.logout(&config, &cancellation).await?;

    assert_eq!(
        outcome,
        CodexNativeLogout::RevocationFailed(CodexRevocationFailure::Transport)
    );
    assert!(KeyStore::get_oauth(&config, "Codex")?.is_none());
    Ok(())
}

fn native_config(environment: &TestEnvironment) -> AppConfig {
    AppConfig {
        key_file: Some(
            environment
                .root
                .join("config/tcui/keys.toml")
                .display()
                .to_string(),
        ),
        ..AppConfig::default()
    }
}

fn store_native_credential(
    config: &AppConfig,
) -> Result<(), crate::config::key_store::KeyStoreError> {
    KeyStore::upsert_oauth(
        config,
        &OAuthCredential {
            provider: "Codex".to_string(),
            access_token: "native-access".to_string(),
            refresh_token: Some("native-refresh".to_string()),
            expires_at: Utc::now() + ChronoDuration::hours(1),
            account_id: Some("native-account".to_string()),
            ownership: OAuthCredentialOwnership::Tcui,
            source: OAuthCredentialSource::NativeOAuth,
        },
    )
}

fn fixture(
    endpoint: &str,
    timeout: Duration,
) -> Result<CodexNativeAdapter, Box<dyn std::error::Error>> {
    Ok(CodexNativeAdapter::fixture_with_revocation(
        "https://authorize.example.test/oauth/authorize",
        endpoint,
        endpoint,
        endpoint,
        endpoint,
        timeout,
    )?)
}

async fn revoke_server(
    status: &str,
    body: &str,
) -> Result<
    (
        String,
        tokio::task::JoinHandle<Result<String, std::io::Error>>,
    ),
    std::io::Error,
> {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let endpoint = format!("http://{}", listener.local_addr()?);
    let status = status.to_string();
    let body = body.to_string();
    let server = tokio::spawn(async move {
        let (mut socket, _) = listener.accept().await?;
        let mut request = vec![0_u8; 4096];
        let length = socket.read(&mut request).await?;
        let response = format!(
            "HTTP/1.1 {status}\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
            body.len()
        );
        socket.write_all(response.as_bytes()).await?;
        String::from_utf8(request[..length].to_vec())
            .map_err(|_| std::io::Error::other("request was not utf-8"))
    });
    Ok((endpoint, server))
}

async fn delayed_revoke_server() -> Result<(String, tokio::task::JoinHandle<()>), std::io::Error> {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let endpoint = format!("http://{}", listener.local_addr()?);
    let server = tokio::spawn(async move {
        let _socket = listener.accept().await;
        tokio::time::sleep(Duration::from_secs(1)).await;
    });
    Ok((endpoint, server))
}
