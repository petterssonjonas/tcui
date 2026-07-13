#![expect(
    clippy::await_holding_lock,
    reason = "Tests serialize process-global HOME and XDG fixture paths through async OAuth flows."
)]

use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

use super::codex::{CodexCredentialSource, CodexNativeAdapter};
use super::codex_test_support::TestEnvironment;
use crate::config::key_store::{OAuthCredentialOwnership, OAuthCredentialSource};
use crate::config::{AppConfig, KeyStore};
use crate::llm::auth::oauth::oauth_cancellation;

#[tokio::test]
async fn native_device_login_reports_missing_codex_entitlement()
-> Result<(), Box<dyn std::error::Error>> {
    let _guard = crate::test_support::env_lock()
        .lock()
        .expect("environment lock poisoned");
    let environment = TestEnvironment::new("codex-native-device-entitlement")?;
    let (base, server) = entitlement_server().await?;
    let adapter = CodexNativeAdapter::fixture(
        "https://authorize.example.test/oauth/authorize",
        &format!("{base}/token"),
        &format!("{base}/device/start"),
        &format!("{base}/device/poll"),
    )?;
    let (cancellation, _) = oauth_cancellation();

    let authorization = adapter.begin_device(&cancellation).await?;
    let error = adapter
        .complete_device(&AppConfig::default(), authorization, &cancellation)
        .await
        .unwrap_err();
    let requests = server.await??;

    assert!(matches!(
        error,
        super::codex::CodexNativeError::MissingEntitlement
    ));
    assert_eq!(requests.len(), 2);
    assert!(requests[1].contains("/device/poll"));
    assert!(!environment.root.join("data/tcui/keys.toml").exists());
    Ok(())
}

#[tokio::test]
async fn native_device_login_exchanges_user_code_and_persists_encrypted_credential()
-> Result<(), Box<dyn std::error::Error>> {
    let _guard = crate::test_support::env_lock()
        .lock()
        .expect("environment lock poisoned");
    let environment = TestEnvironment::new("codex-native-device")?;
    let account_claims = URL_SAFE_NO_PAD
        .encode(r#"{"https://api.openai.com/auth":{"chatgpt_account_id":"device-account"}}"#);
    let (base, server) = device_server(account_claims).await?;
    let adapter = CodexNativeAdapter::fixture(
        "https://authorize.example.test/oauth/authorize",
        &format!("{base}/token"),
        &format!("{base}/device/start"),
        &format!("{base}/device/poll"),
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
    let (cancellation, _) = oauth_cancellation();

    let device = adapter.begin_device(&cancellation).await?;
    assert_eq!(device.user_code(), "ABCD-EFGH");
    assert_eq!(
        device.verification_url(),
        "https://auth.openai.com/codex/device"
    );
    let credential = adapter
        .complete_device(&config, device, &cancellation)
        .await?;
    let requests = server.await??;

    assert_eq!(credential.source(), CodexCredentialSource::TcuiNative);
    assert_eq!(credential.account_id(), Some("device-account"));
    assert!(requests[0].contains("/device/start"));
    assert!(requests[1].contains("/device/poll"));
    assert!(requests[2].contains("grant_type=authorization_code"));
    let stored = KeyStore::get_oauth(&config, "Codex")?
        .ok_or_else(|| std::io::Error::other("missing native device credential"))?;
    assert_eq!(stored.provider, "Codex");
    assert_eq!(stored.access_token, "device-access");
    assert_eq!(stored.refresh_token.as_deref(), Some("device-refresh"));
    assert_eq!(stored.account_id.as_deref(), Some("device-account"));
    assert_eq!(stored.ownership, OAuthCredentialOwnership::Tcui);
    assert_eq!(stored.source, OAuthCredentialSource::NativeOAuth);
    let raw_store = std::fs::read_to_string(environment.root.join("config/tcui/keys.toml"))?;
    assert!(!raw_store.contains("device-access"));
    assert!(!raw_store.contains("device-refresh"));
    Ok(())
}

async fn device_server(
    account_claims: String,
) -> Result<
    (
        String,
        tokio::task::JoinHandle<Result<Vec<String>, std::io::Error>>,
    ),
    std::io::Error,
> {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let base = format!("http://{}", listener.local_addr()?);
    let server = tokio::spawn(async move {
        let mut requests = Vec::new();
        for body in [
            r#"{"device_auth_id":"device-id","user_code":"ABCD-EFGH","interval":1}"#.to_string(),
            r#"{"authorization_code":"device-code","code_verifier":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"}"#.to_string(),
            format!(
                r#"{{"access_token":"device-access","refresh_token":"device-refresh","id_token":"header.{account_claims}.signature","token_type":"Bearer","expires_in":3600}}"#
            ),
        ] {
            let (mut socket, _) = listener.accept().await?;
            let mut request = vec![0_u8; 4096];
            let length = socket.read(&mut request).await?;
            let response = format!(
                "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{}",
                body.len(),
                body
            );
            socket.write_all(response.as_bytes()).await?;
            requests.push(
                String::from_utf8(request[..length].to_vec())
                    .map_err(|_| std::io::Error::other("request was not utf-8"))?,
            );
        }
        Ok(requests)
    });
    Ok((base, server))
}

async fn entitlement_server() -> Result<
    (
        String,
        tokio::task::JoinHandle<Result<Vec<String>, std::io::Error>>,
    ),
    std::io::Error,
> {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let base = format!("http://{}", listener.local_addr()?);
    let server = tokio::spawn(async move {
        let mut requests = Vec::new();
        for body in [
            r#"{"device_auth_id":"device-id","user_code":"ABCD-EFGH","interval":1}"#,
            r#"{"error_code":"access_denied","error_description":"missing_codex_entitlement"}"#,
        ] {
            let (mut socket, _) = listener.accept().await?;
            let mut request = vec![0_u8; 4096];
            let length = socket.read(&mut request).await?;
            let response = format!(
                "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
                body.len(),
            );
            socket.write_all(response.as_bytes()).await?;
            requests.push(
                String::from_utf8(request[..length].to_vec())
                    .map_err(|_| std::io::Error::other("request was not utf-8"))?,
            );
        }
        Ok(requests)
    });
    Ok((base, server))
}
