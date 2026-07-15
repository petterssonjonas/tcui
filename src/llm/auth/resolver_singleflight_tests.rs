#![expect(
    clippy::await_holding_lock,
    reason = "The fixture serializes process-global HOME and XDG paths across the refresh."
)]

use secrecy::ExposeSecret;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;

use chrono::{Duration as ChronoDuration, Utc};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use tokio::sync::{mpsc, oneshot, Barrier};

use super::resolver::resolve_provider_credential_with_native_adapter;
use super::resolver_tests::NativeTestEnvironment;
use super::CredentialRequest;
use crate::config::key_store::{OAuthCredential, OAuthCredentialOwnership, OAuthCredentialSource};
use crate::config::KeyStore;

const SURVIVING_CALLERS: usize = 16;

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn cancelled_refresh_leader_keeps_singleflight_alive_for_all_parallel_waiters() {
    // Given
    let _guard = crate::test_support::env_lock()
        .lock()
        .expect("environment lock");
    let environment = NativeTestEnvironment::new("resolver-singleflight-cancellation");
    let config = environment.config();
    KeyStore::upsert_oauth(
        &config,
        &OAuthCredential {
            provider: "Codex".to_owned(),
            access_token: "expired-access".to_owned(),
            refresh_token: Some("refresh-token".to_owned()),
            expires_at: Utc::now() - ChronoDuration::minutes(1),
            account_id: Some("account-123".to_owned()),
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
    let request_count = Arc::clone(&requests);
    let (started_tx, started_rx) = oneshot::channel();
    let (release_tx, release_rx) = oneshot::channel();
    let fixture = tokio::spawn(async move {
        let (mut stream, _) = listener.accept().await.expect("accept token request");
        let mut request = [0_u8; 2_048];
        let _ = stream.read(&mut request).await.expect("read token request");
        request_count.fetch_add(1, Ordering::SeqCst);
        started_tx.send(()).expect("signal refresh started");
        release_rx.await.expect("release token response");
        let body = r#"{"access_token":"refreshed-access","refresh_token":"refreshed-refresh","token_type":"Bearer","expires_in":3600}"#;
        let response = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        );
        let _ = stream.write_all(response.as_bytes()).await;
    });
    let leader_config = config.clone();
    let leader_adapter = adapter.clone();
    let leader = tokio::spawn(async move {
        resolve_provider_credential_with_native_adapter(request(), &leader_config, &leader_adapter)
            .await
    });
    started_rx.await.expect("refresh request should start");
    let barrier = Arc::new(Barrier::new(SURVIVING_CALLERS + 1));
    let (ready_tx, mut ready_rx) = mpsc::channel(SURVIVING_CALLERS);
    let mut waiters = Vec::with_capacity(SURVIVING_CALLERS);
    for _ in 0..SURVIVING_CALLERS {
        let waiter_config = config.clone();
        let waiter_adapter = adapter.clone();
        let waiter_barrier = Arc::clone(&barrier);
        let waiter_ready = ready_tx.clone();
        waiters.push(tokio::spawn(async move {
            waiter_barrier.wait().await;
            waiter_ready.send(()).await.expect("signal waiter ready");
            resolve_provider_credential_with_native_adapter(
                request(),
                &waiter_config,
                &waiter_adapter,
            )
            .await
        }));
    }
    drop(ready_tx);
    barrier.wait().await;
    for _ in 0..SURVIVING_CALLERS {
        ready_rx.recv().await.expect("all waiters should start");
    }
    tokio::task::yield_now().await;

    // When
    leader.abort();
    assert!(
        leader
            .await
            .expect_err("leader should be cancelled")
            .is_cancelled(),
        "leader task should observe cancellation"
    );
    release_tx.send(()).expect("release token response");
    fixture.await.expect("finish token fixture");
    let results = tokio::time::timeout(Duration::from_secs(2), futures::future::join_all(waiters))
        .await
        .expect("singleflight waiters must not hang after leader cancellation");

    // Then
    assert_eq!(requests.load(Ordering::SeqCst), 1);
    for result in results {
        let credential = result
            .expect("waiter task should finish")
            .expect("waiter resolution should succeed")
            .expect("refreshed credential should exist");
        assert_eq!(
            credential.bearer_token().expose_secret(),
            "refreshed-access"
        );
    }
}

const fn request() -> CredentialRequest<'static> {
    CredentialRequest::new(
        "Codex",
        "OPENAI_API_KEY",
        "https://chatgpt.com/backend-api/codex",
    )
}
