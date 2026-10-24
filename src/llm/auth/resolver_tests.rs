#![expect(
    clippy::await_holding_lock,
    reason = "Resolver fixtures isolate process-global HOME and XDG paths across async refreshes."
)]

use super::{CredentialError, CredentialRequest};
use chrono::{Duration, Utc};
use secrecy::ExposeSecret;
use std::ffi::OsString;
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use tokio::sync::oneshot;

use crate::config::key_store::{OAuthCredential, OAuthCredentialOwnership, OAuthCredentialSource};
use crate::config::{AppConfig, KeyStore};

pub(super) struct NativeTestEnvironment {
    root: PathBuf,
    original_home: Option<OsString>,
    original_data_home: Option<OsString>,
    original_config_home: Option<OsString>,
    original_cache_home: Option<OsString>,
}

impl NativeTestEnvironment {
    pub(super) fn new(label: &str) -> Self {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("system clock")
            .as_nanos();
        let root = std::env::temp_dir().join(format!("tcui-{label}-{nanos}"));
        std::fs::create_dir_all(&root).expect("create test root");
        let original_home = std::env::var_os("HOME");
        let original_data_home = std::env::var_os("XDG_DATA_HOME");
        let original_config_home = std::env::var_os("XDG_CONFIG_HOME");
        let original_cache_home = std::env::var_os("XDG_CACHE_HOME");
        std::env::set_var("HOME", &root);
        std::env::set_var("XDG_DATA_HOME", root.join("data"));
        std::env::set_var("XDG_CONFIG_HOME", root.join("config"));
        std::env::set_var("XDG_CACHE_HOME", root.join("cache"));
        KeyStore::initialize_vault(&AppConfig::default(), "resolver fixture password")
            .expect("initialize encrypted fixture vault");
        Self {
            root,
            original_home,
            original_data_home,
            original_config_home,
            original_cache_home,
        }
    }

    pub(super) fn config(&self) -> AppConfig {
        AppConfig {
            key_file: Some(self.root.join("keys.toml").display().to_string()),
            ..AppConfig::default()
        }
    }
}

impl Drop for NativeTestEnvironment {
    fn drop(&mut self) {
        let _ = crate::protection::lock();
        match self.original_home.take() {
            Some(home) => std::env::set_var("HOME", home),
            None => std::env::remove_var("HOME"),
        }
        match self.original_data_home.take() {
            Some(data_home) => std::env::set_var("XDG_DATA_HOME", data_home),
            None => std::env::remove_var("XDG_DATA_HOME"),
        }
        match self.original_config_home.take() {
            Some(config_home) => std::env::set_var("XDG_CONFIG_HOME", config_home),
            None => std::env::remove_var("XDG_CONFIG_HOME"),
        }
        match self.original_cache_home.take() {
            Some(cache_home) => std::env::set_var("XDG_CACHE_HOME", cache_home),
            None => std::env::remove_var("XDG_CACHE_HOME"),
        }
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

#[tokio::test]
async fn resolver_uses_only_explicitly_saved_keys() {
    let _guard = crate::test_support::env_lock().lock().unwrap();
    let environment = NativeTestEnvironment::new("resolver-explicit-key");
    let config = environment.config();
    KeyStore::set(&config, "OpenRouter", "saved-key").unwrap();
    let credential = super::resolver::resolve_provider_credential_with_config(
        CredentialRequest::new("OpenRouter", "IGNORED", "https://openrouter.ai/api/v1"),
        &config,
    )
    .await
    .unwrap()
    .unwrap();
    assert_eq!(credential.bearer_token().expose_secret(), "saved-key");
}

#[tokio::test]
async fn provider_credentials_remain_available_while_configuration_is_locked() {
    let _guard = crate::test_support::env_lock().lock().unwrap();
    let environment = NativeTestEnvironment::new("resolver-config-contention");
    let config = environment.config();
    config.save().unwrap();
    KeyStore::set(&config, "OpenRouter", "stored-during-config-lock").unwrap();
    let lock_path = environment.root.join("config/tcui/.config.lock");
    let lock = std::fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(lock_path)
        .unwrap();
    lock.try_lock().unwrap();
    assert!(AppConfig::load().is_err());
    let credential = super::resolve_provider_credential(CredentialRequest::new(
        "OpenRouter",
        "IGNORED",
        "https://openrouter.ai/api/v1",
    ))
    .await
    .unwrap()
    .unwrap();
    assert_eq!(
        credential.bearer_token().expose_secret(),
        "stored-during-config-lock"
    );
}

#[tokio::test]
async fn resolver_rejects_custom_native_oauth_endpoints() {
    let result = super::resolve_provider_credential(CredentialRequest::new(
        "OpenRouter OAuth",
        "IGNORED",
        "https://custom.example/v1",
    ))
    .await;
    assert!(matches!(result, Err(CredentialError::UntrustedEndpoint)));
}

#[tokio::test]
async fn resolver_refreshes_an_expired_native_credential_once_for_parallel_callers() {
    // Given
    let _guard = crate::test_support::env_lock()
        .lock()
        .expect("environment lock");
    let environment = NativeTestEnvironment::new("resolver-singleflight");
    let config = environment.config();
    KeyStore::upsert_oauth(
        &config,
        &OAuthCredential {
            provider: "Codex".to_string(),
            access_token: "expired-access".to_string(),
            refresh_token: Some("refresh-token".to_string()),
            expires_at: Utc::now() - Duration::minutes(1),
            account_id: Some("account-123".to_string()),
            ownership: OAuthCredentialOwnership::Tcui,
            source: OAuthCredentialSource::NativeOAuth,
        },
    )
    .expect("store expired native credential");
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind token fixture");
    let endpoint = format!("http://{}", listener.local_addr().expect("fixture address"));
    let adapter = super::codex::CodexNativeAdapter::fixture(
        "https://authorize.example.test/oauth/authorize",
        &endpoint,
        &endpoint,
        &endpoint,
    )
    .expect("create native adapter fixture");
    let requests = Arc::new(AtomicUsize::new(0));
    let (started_tx, mut started_rx) = oneshot::channel();
    let (release_tx, release_rx) = oneshot::channel();
    let request_count = Arc::clone(&requests);
    let fixture = tokio::spawn(async move {
        let (mut stream, _) = listener.accept().await.expect("accept token request");
        let mut request = [0_u8; 2048];
        let _ = stream.read(&mut request).await.expect("read token request");
        request_count.fetch_add(1, Ordering::SeqCst);
        started_tx.send(()).expect("signal refresh started");
        release_rx.await.expect("release token response");
        let body = r#"{"access_token":"refreshed-access","refresh_token":"refreshed-refresh","token_type":"Bearer","expires_in":3600}"#;
        let response = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        );
        stream
            .write_all(response.as_bytes())
            .await
            .expect("write token response");
    });
    let first_request = CredentialRequest::new(
        "Codex",
        "OPENAI_API_KEY",
        "https://chatgpt.com/backend-api/codex",
    );
    let first = super::resolver::resolve_provider_credential_with_native_adapter(
        first_request,
        &config,
        &adapter,
    );
    tokio::pin!(first);

    // When
    tokio::select! {
        _ = &mut first => panic!("refresh completed before fixture release"),
        _ = &mut started_rx => {}
    }
    let second_request = CredentialRequest::new(
        "Codex",
        "OPENAI_API_KEY",
        "https://chatgpt.com/backend-api/codex",
    );
    let second = super::resolver::resolve_provider_credential_with_native_adapter(
        second_request,
        &config,
        &adapter,
    );
    tokio::pin!(second);
    assert!(matches!(
        futures::poll!(&mut second),
        std::task::Poll::Pending
    ));
    release_tx.send(()).expect("release refresh response");
    let (first, second) = tokio::join!(first, second);

    // Then
    assert_eq!(
        first
            .expect("first resolution")
            .expect("first credential")
            .bearer_token()
            .expose_secret(),
        "refreshed-access"
    );
    assert_eq!(
        second
            .expect("second resolution")
            .expect("second credential")
            .bearer_token()
            .expose_secret(),
        "refreshed-access"
    );
    fixture.await.expect("finish token fixture");
    assert_eq!(requests.load(Ordering::SeqCst), 1);
}
