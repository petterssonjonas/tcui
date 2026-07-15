#![expect(
    clippy::await_holding_lock,
    reason = "Resolver fixtures isolate process-global HOME and XDG paths across async refreshes."
)]

use super::{CredentialError, CredentialRequest};
use chrono::{Duration, Utc};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use tokio::sync::oneshot;

use super::resolver_tests::NativeTestEnvironment;
use crate::config::key_store::{OAuthCredential, OAuthCredentialOwnership, OAuthCredentialSource};
use crate::config::KeyStore;

#[tokio::test]
async fn resolver_propagates_one_native_refresh_failure_to_parallel_callers() {
    // Given
    let _guard = crate::test_support::env_lock()
        .lock()
        .expect("environment lock");
    let environment = NativeTestEnvironment::new("resolver-refresh-failure");
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
        stream
            .write_all(
                b"HTTP/1.1 401 Unauthorized\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
            )
            .await
            .expect("write rejected token response");
    });
    let first = super::resolver::resolve_provider_credential_with_native_adapter(
        CredentialRequest::new(
            "Codex",
            "OPENAI_API_KEY",
            "https://chatgpt.com/backend-api/codex",
        ),
        &config,
        &adapter,
    );
    tokio::pin!(first);

    // When
    tokio::select! {
        _ = &mut first => panic!("refresh completed before fixture release"),
        _ = &mut started_rx => {}
    }
    let second = super::resolver::resolve_provider_credential_with_native_adapter(
        CredentialRequest::new(
            "Codex",
            "OPENAI_API_KEY",
            "https://chatgpt.com/backend-api/codex",
        ),
        &config,
        &adapter,
    );
    tokio::pin!(second);
    assert!(matches!(
        futures::poll!(&mut second),
        std::task::Poll::Pending
    ));
    release_tx.send(()).expect("release rejected response");
    let (first, second) = tokio::join!(first, second);

    // Then
    assert!(matches!(first, Err(CredentialError::NativeRefreshFailed)));
    assert!(matches!(second, Err(CredentialError::NativeRefreshFailed)));
    fixture.await.expect("finish token fixture");
    assert_eq!(requests.load(Ordering::SeqCst), 1);
}
