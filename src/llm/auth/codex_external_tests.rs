#![expect(
    clippy::await_holding_lock,
    reason = "The native logout fixture serializes process-global HOME and XDG paths."
)]

use chrono::{Duration, Utc};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

use super::codex::{
    codex_status, read_external_credential, resolve_credential, CodexCredentialSource,
    CodexNativeAdapter,
};
use super::codex_test_support::TestEnvironment;
use crate::config::key_store::{OAuthCredential, OAuthCredentialOwnership, OAuthCredentialSource};
use crate::config::{AppConfig, KeyStore};

// Todo 5 covers the native adapter's bounded cancellation and source-aware status APIs. Todo 7
// owns real terminal SIGINT/process-group event wiring and dispatching that status through the CLI.

#[test]
fn external_legacy_auth_is_read_in_place_without_secret_debug_output(
) -> Result<(), Box<dyn std::error::Error>> {
    let _guard = crate::test_support::env_lock()
        .lock()
        .expect("environment lock poisoned");
    let environment = TestEnvironment::new("codex-external-auth")?;
    let contents = r#"{"tokens":{"access_token":"external-access-token","refresh_token":"external-refresh-token","account_id":"account-123"}}"#;
    environment.write_external_auth(contents)?;

    let credential = read_external_credential()?
        .ok_or_else(|| std::io::Error::other("missing parsed external credential"))?;

    assert_eq!(credential.source(), CodexCredentialSource::ExternalCli);
    assert_eq!(credential.account_id(), Some("account-123"));
    assert_eq!(std::fs::read_to_string(environment.auth_path())?, contents);
    assert!(!format!("{credential:?}").contains("external-access-token"));
    assert!(!format!("{credential:?}").contains("external-refresh-token"));
    Ok(())
}

#[test]
fn external_current_auth_uses_top_level_account_without_copying_file(
) -> Result<(), Box<dyn std::error::Error>> {
    let _guard = crate::test_support::env_lock()
        .lock()
        .expect("environment lock poisoned");
    let environment = TestEnvironment::new("codex-current-external-auth")?;
    let contents = r#"{"version":1,"provider":"openai","account":"user@example.test","oauth_token_set":{"access_token":"current-access","refresh_token":"current-refresh"}}"#;
    environment.write_external_auth(contents)?;

    let credential = read_external_credential()?
        .ok_or_else(|| std::io::Error::other("missing parsed external credential"))?;

    assert_eq!(credential.account_id(), Some("user@example.test"));
    assert_eq!(std::fs::read_to_string(environment.auth_path())?, contents);
    assert!(!format!("{credential:?}").contains("current-access"));
    Ok(())
}

#[test]
fn tcui_native_credential_has_precedence_without_mutating_external_auth(
) -> Result<(), Box<dyn std::error::Error>> {
    let _guard = crate::test_support::env_lock()
        .lock()
        .expect("environment lock poisoned");
    let environment = TestEnvironment::new("codex-native-precedence")?;
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
    let external =
        r#"{"tokens":{"access_token":"external-access","account_id":"external-account"}}"#;
    environment.write_external_auth(external)?;
    KeyStore::upsert_oauth(
        &config,
        &OAuthCredential {
            provider: "Codex".to_string(),
            access_token: "native-access".to_string(),
            refresh_token: Some("native-refresh".to_string()),
            expires_at: Utc::now() + Duration::hours(1),
            account_id: Some("native-account".to_string()),
            ownership: OAuthCredentialOwnership::Tcui,
            source: OAuthCredentialSource::NativeOAuth,
        },
    )?;

    let credential = resolve_credential(&config)?
        .ok_or_else(|| std::io::Error::other("missing preferred credential"))?;

    assert_eq!(credential.source(), CodexCredentialSource::TcuiNative);
    assert_eq!(credential.account_id(), Some("native-account"));
    assert_eq!(std::fs::read_to_string(environment.auth_path())?, external);
    Ok(())
}

#[tokio::test]
async fn native_status_is_redacted_and_ordinary_logout_preserves_external_auth(
) -> Result<(), Box<dyn std::error::Error>> {
    let _guard = crate::test_support::env_lock()
        .lock()
        .expect("environment lock poisoned");
    let environment = TestEnvironment::new("codex-native-logout")?;
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
    let external =
        r#"{"tokens":{"access_token":"external-access","account_id":"external-account"}}"#;
    environment.write_external_auth(external)?;
    KeyStore::upsert_oauth(
        &config,
        &OAuthCredential {
            provider: "Codex".to_string(),
            access_token: "native-access".to_string(),
            refresh_token: Some("native-refresh".to_string()),
            expires_at: Utc::now() + Duration::hours(1),
            account_id: Some("native-account".to_string()),
            ownership: OAuthCredentialOwnership::Tcui,
            source: OAuthCredentialSource::NativeOAuth,
        },
    )?;

    let status = codex_status(&config)?;
    let (endpoint, server) = revoke_server().await?;
    let (cancellation, _) = crate::llm::auth::oauth::oauth_cancellation();
    let removed = CodexNativeAdapter::fixture_with_revocation(
        "https://authorize.example.test/oauth/authorize",
        &endpoint,
        &endpoint,
        &endpoint,
        &endpoint,
        std::time::Duration::from_secs(1),
    )?
    .logout(&config, &cancellation)
    .await?;
    server.await??;

    assert!(status.to_string().contains("source=tcui-native"));
    assert!(!status.to_string().contains("native-access"));
    assert_eq!(removed, super::codex::CodexNativeLogout::Revoked);
    assert_eq!(std::fs::read_to_string(environment.auth_path())?, external);
    assert!(KeyStore::get_oauth(&config, "Codex")?.is_none());
    Ok(())
}

async fn revoke_server(
) -> Result<(String, tokio::task::JoinHandle<Result<(), std::io::Error>>), std::io::Error> {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let endpoint = format!("http://{}", listener.local_addr()?);
    let server = tokio::spawn(async move {
        let (mut socket, _) = listener.accept().await?;
        let mut request = vec![0_u8; 4096];
        let _ = socket.read(&mut request).await?;
        socket
            .write_all(
                b"HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: 2\r\nconnection: close\r\n\r\n{}",
            )
            .await
    });
    Ok((endpoint, server))
}
