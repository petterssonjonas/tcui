#![expect(
    clippy::await_holding_lock,
    reason = "Refresh lifecycle fixtures serialize isolated process-global HOME and XDG paths."
)]

use chrono::{Duration as ChronoDuration, Utc};
use secrecy::ExposeSecret;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use tokio::sync::oneshot;
use tokio::task::AbortHandle;

use super::*;
use crate::config::key_store::{
    CredentialStorage, OAuthCredential, OAuthCredentialOwnership, OAuthCredentialSource,
};
use crate::llm::auth::resolver_tests::NativeTestEnvironment;
use crate::protection::ProtectionPolicy;

#[tokio::test]
async fn shutdown_joins_refresh_after_the_last_probe_is_cancelled() {
    let _guard = crate::test_support::env_lock()
        .lock()
        .expect("environment lock");
    let environment = NativeTestEnvironment::new("refresh-shutdown");
    let config = environment.config();
    let original = expired_credential("Codex");
    KeyStore::upsert_oauth(&config, &original).expect("store expired credential");
    let mut server = GatedRefresh::new("stale-access").await;
    let probe = spawn_refresh(&config, &server.adapter, "Codex");
    server.wait_started().await;
    let (_, worker) = flight_identity("Codex");

    probe.abort();
    assert!(probe.await.expect_err("probe cancelled").is_cancelled());
    shutdown().await;

    assert!(
        worker.is_finished(),
        "shutdown must drop the credential-bearing worker"
    );
    assert!(!native_refresh_flights().contains_key(&refresh_root()));
    server.release().await;
    assert_eq!(
        KeyStore::get_oauth(&config, "Codex").expect("read credential"),
        Some(original)
    );
}

#[tokio::test]
async fn lock_unlock_cannot_resume_a_refresh_from_the_previous_session() {
    let _guard = crate::test_support::env_lock()
        .lock()
        .expect("environment lock");
    for storage in [CredentialStorage::Encrypted, CredentialStorage::Plaintext] {
        let environment = NativeTestEnvironment::new("refresh-lock-unlock");
        let config = environment.config();
        crate::protection::configure(
            ProtectionPolicy {
                app_password: true,
                credentials: storage == CredentialStorage::Encrypted,
                ..ProtectionPolicy::default()
            },
            "resolver fixture password",
        )
        .expect("configure protected application");
        let original = expired_credential("Codex");
        KeyStore::upsert_oauth_with_storage(&config, &original, storage)
            .expect("store expired credential");
        let mut server = GatedRefresh::new("stale-access").await;
        let probe = spawn_refresh(&config, &server.adapter, "Codex");
        server.wait_started().await;
        let (session, worker) = flight_identity("Codex");

        shutdown().await;
        crate::protection::lock().expect("revoke application keys");
        if storage == CredentialStorage::Encrypted {
            assert!(KeyStore::get_oauth(&config, "Codex").is_err());
        } else {
            assert_eq!(
                KeyStore::get_oauth(&config, "Codex")
                    .expect("plaintext credential remains readable"),
                Some(original.clone())
            );
        }
        crate::protection::unlock("resolver fixture password").expect("unlock new session");
        server.release().await;

        assert!(worker.is_finished());
        assert!(
            !session.load(Ordering::Acquire),
            "old session must remain invalid after unlock"
        );
        assert!(matches!(
            probe_result(probe).await,
            Err(CredentialError::NativeRefreshInterrupted)
        ));
        assert_eq!(
            KeyStore::get_oauth(&config, "Codex").expect("read retained credential"),
            Some(original)
        );
        assert_eq!(
            KeyStore::provider_storage(&config, "Codex").expect("connection metadata"),
            Some(storage)
        );
    }
}

#[tokio::test]
async fn delayed_refresh_does_not_recreate_a_deleted_connection_without_invalidation() {
    let _guard = crate::test_support::env_lock()
        .lock()
        .expect("environment lock");
    let environment = NativeTestEnvironment::new("refresh-deleted-cas");
    let config = environment.config();
    KeyStore::upsert_oauth(&config, &expired_credential("Codex"))
        .expect("store expired credential");
    let mut server = GatedRefresh::new("stale-access").await;
    let probe = spawn_refresh(&config, &server.adapter, "Codex");
    server.wait_started().await;

    assert!(KeyStore::remove_oauth(&config, "Codex").expect("delete connection"));
    server.release().await;

    assert!(matches!(
        probe_result(probe).await,
        Err(CredentialError::NativeRefreshFailed)
    ));
    assert!(KeyStore::get_oauth(&config, "Codex")
        .expect("read removed connection")
        .is_none());
    shutdown().await;
}

#[tokio::test]
async fn delayed_refresh_does_not_overwrite_a_replacement_without_invalidation() {
    let _guard = crate::test_support::env_lock()
        .lock()
        .expect("environment lock");
    let environment = NativeTestEnvironment::new("refresh-replaced-cas");
    let config = environment.config();
    let mut replacement = expired_credential("Codex");
    KeyStore::upsert_oauth(&config, &replacement).expect("store expired credential");
    let mut server = GatedRefresh::new("stale-access").await;
    let probe = spawn_refresh(&config, &server.adapter, "Codex");
    server.wait_started().await;
    replacement.access_token = "reconnected-access".into();
    replacement.refresh_token = Some("reconnected-refresh".into());
    replacement.expires_at = Utc::now() + ChronoDuration::hours(1);

    KeyStore::upsert_oauth(&config, &replacement).expect("replace connection");
    server.release().await;

    assert!(matches!(
        probe_result(probe).await,
        Err(CredentialError::NativeRefreshFailed)
    ));
    assert_eq!(
        KeyStore::get_oauth(&config, "Codex").expect("read replacement"),
        Some(replacement)
    );
    shutdown().await;
}

#[tokio::test]
async fn reconnect_invalidates_the_old_generation_without_cancelling_a_sibling_or_new_flight() {
    let _guard = crate::test_support::env_lock()
        .lock()
        .expect("environment lock");
    let environment = NativeTestEnvironment::new("refresh-reconnect-generation");
    let config = environment.config();
    let original = expired_credential("Codex");
    KeyStore::upsert_oauth(&config, &original).expect("store primary credential");
    KeyStore::upsert_oauth_with_storage(
        &config,
        &expired_credential("Codex::plaintext"),
        CredentialStorage::Plaintext,
    )
    .expect("store plaintext sibling");
    let mut old_server = GatedRefresh::new("stale-access").await;
    let old_probe = spawn_refresh(&config, &old_server.adapter, "Codex");
    old_server.wait_started().await;
    let (old_generation, old_worker) = flight_identity("Codex");
    let mut sibling_server = GatedRefresh::new("sibling-access").await;
    let sibling_probe = spawn_refresh(&config, &sibling_server.adapter, "Codex::plaintext");
    sibling_server.wait_started().await;
    let (sibling_generation, _) = flight_identity("Codex::plaintext");

    invalidate_provider_refreshes("Codex");
    assert!(KeyStore::remove_oauth(&config, "Codex").expect("remove old connection"));
    // Identical tokens exercise reconnect ABA: record equality alone cannot identify the session.
    KeyStore::upsert_oauth(&config, &original).expect("reconnect same account");
    let mut new_server = GatedRefresh::new("reconnected-access").await;
    let new_probe = spawn_refresh(&config, &new_server.adapter, "Codex");
    new_server.wait_started().await;
    let (new_generation, _) = flight_identity("Codex");
    old_server.release().await;
    assert!(matches!(
        probe_result(old_probe).await,
        Err(CredentialError::NativeRefreshInterrupted)
    ));
    assert!(!old_generation.load(Ordering::Acquire));
    assert!(new_generation.load(Ordering::Acquire));
    assert!(sibling_generation.load(Ordering::Acquire));
    new_server.release().await;
    sibling_server.release().await;

    assert_eq!(bearer(new_probe).await, "reconnected-access");
    assert_eq!(bearer(sibling_probe).await, "sibling-access");
    shutdown().await;
    assert!(
        old_worker.is_finished(),
        "shutdown must also join previously invalidated workers"
    );
    assert_eq!(
        KeyStore::get_oauth(&config, "Codex")
            .expect("read primary")
            .expect("primary exists")
            .access_token,
        "reconnected-access"
    );
    assert_eq!(
        KeyStore::get_oauth(&config, "Codex::plaintext")
            .expect("read sibling")
            .expect("sibling exists")
            .access_token,
        "sibling-access"
    );
    assert_eq!(
        KeyStore::provider_storage(&config, "Codex::plaintext").expect("sibling metadata"),
        Some(CredentialStorage::Plaintext)
    );
}

#[tokio::test]
async fn rejected_unexpired_tokens_refresh_in_the_owned_registry_and_stop_before_revocation() {
    let _guard = crate::test_support::env_lock()
        .lock()
        .expect("environment lock");
    let environment = NativeTestEnvironment::new("refresh-rejected-unexpired");
    let config = environment.config();
    let mut original = expired_credential("Codex");
    original.expires_at = Utc::now() + ChronoDuration::hours(1);
    KeyStore::upsert_oauth(&config, &original).expect("store unexpired credential");
    let mut server = GatedRefresh::new("stale-access").await;
    let probe = spawn_rejected_refresh(&config, &server.adapter, "expired-access");
    server.wait_started().await;
    let (_, worker) = flight_identity("Codex");

    shutdown().await;
    server.release().await;

    assert!(worker.is_finished());
    assert!(matches!(
        probe_result(probe).await,
        Err(CredentialError::NativeRefreshInterrupted)
    ));
    assert_eq!(
        KeyStore::get_oauth(&config, "Codex").expect("read retained credential"),
        Some(original)
    );
}

#[tokio::test]
async fn rejected_previous_tokens_reuse_a_newer_unexpired_credential_without_refreshing_it() {
    let _guard = crate::test_support::env_lock()
        .lock()
        .expect("environment lock");
    let environment = NativeTestEnvironment::new("refresh-rejected-replaced");
    let config = environment.config();
    let mut replacement = expired_credential("Codex");
    replacement.access_token = "replacement-access".into();
    replacement.expires_at = Utc::now() + ChronoDuration::hours(1);
    KeyStore::upsert_oauth(&config, &replacement).expect("store newer credential");
    let server = GatedRefresh::new("unexpected-refresh").await;

    let probe = spawn_rejected_refresh(&config, &server.adapter, "previous-access");
    assert_eq!(bearer(probe).await, "replacement-access");

    server.server.abort();
    assert!(server
        .server
        .await
        .expect_err("unused token server cancelled")
        .is_cancelled());
    assert_eq!(
        KeyStore::get_oauth(&config, "Codex").expect("read retained replacement"),
        Some(replacement)
    );
    shutdown().await;
}

fn expired_credential(provider: &str) -> OAuthCredential {
    OAuthCredential {
        provider: provider.into(),
        access_token: "expired-access".into(),
        refresh_token: Some("original-refresh".into()),
        expires_at: Utc::now() - ChronoDuration::minutes(1),
        account_id: Some("original-account".into()),
        ownership: OAuthCredentialOwnership::Tcui,
        source: OAuthCredentialSource::NativeOAuth,
    }
}

fn spawn_refresh(
    config: &AppConfig,
    adapter: &CodexNativeAdapter,
    connection: &str,
) -> JoinHandle<Result<Option<Credential>, CredentialError>> {
    let config = config.clone();
    let adapter = adapter.clone();
    let connection = connection.to_owned();
    tokio::spawn(async move {
        resolve_provider_credential_with_native_adapter(
            CredentialRequest::new(
                &connection,
                "IGNORED",
                "https://chatgpt.com/backend-api/codex",
            ),
            &config,
            &adapter,
        )
        .await
    })
}

fn spawn_rejected_refresh(
    config: &AppConfig,
    adapter: &CodexNativeAdapter,
    rejected_access_token: &str,
) -> JoinHandle<Result<Option<Credential>, CredentialError>> {
    let config = config.clone();
    let adapter = adapter.clone();
    let rejected_access_token = rejected_access_token.to_owned();
    tokio::spawn(async move {
        refresh_native_singleflight(
            &config,
            Some(&adapter),
            "Codex",
            Some(&rejected_access_token),
        )
        .await
        .map(|credential| Some(Credential::codex(credential)))
    })
}

fn flight_identity(connection: &str) -> (Arc<AtomicBool>, AbortHandle) {
    let roots = native_refresh_flights();
    let flight = roots
        .get(&refresh_root())
        .and_then(|root| root.flights.get(connection))
        .expect("refresh flight registered");
    (Arc::clone(&flight.current), flight.worker.abort_handle())
}

async fn shutdown() {
    tokio::time::timeout(std::time::Duration::from_secs(2), cancel_all_refreshes())
        .await
        .expect("all owned refresh workers must terminate before revocation");
}

async fn probe_result(
    probe: JoinHandle<Result<Option<Credential>, CredentialError>>,
) -> Result<Option<Credential>, CredentialError> {
    tokio::time::timeout(std::time::Duration::from_secs(2), probe)
        .await
        .expect("refresh waiter must not hang")
        .expect("probe completed")
}

async fn bearer(probe: JoinHandle<Result<Option<Credential>, CredentialError>>) -> String {
    probe_result(probe)
        .await
        .expect("refresh succeeded")
        .expect("credential exists")
        .bearer_token()
        .expose_secret()
        .to_owned()
}

struct GatedRefresh {
    adapter: CodexNativeAdapter,
    started: oneshot::Receiver<()>,
    release: oneshot::Sender<()>,
    server: JoinHandle<()>,
}

impl GatedRefresh {
    async fn new(access_token: &str) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind token fixture");
        let endpoint = format!("http://{}", listener.local_addr().expect("fixture address"));
        let adapter = CodexNativeAdapter::fixture(
            "https://authorize.example.test/oauth/authorize",
            &endpoint,
            &endpoint,
            &endpoint,
        )
        .expect("create native adapter fixture");
        let (started_tx, started) = oneshot::channel();
        let (release, release_rx) = oneshot::channel();
        let access_token = access_token.to_owned();
        let server = tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.expect("accept token request");
            let mut request = Vec::new();
            loop {
                let mut buffer = [0_u8; 2048];
                let length = stream.read(&mut buffer).await.expect("read token request");
                assert_ne!(length, 0, "request must complete before cancellation");
                request.extend_from_slice(&buffer[..length]);
                if let Some(headers_end) = request.windows(4).position(|bytes| bytes == b"\r\n\r\n")
                {
                    let headers =
                        std::str::from_utf8(&request[..headers_end]).expect("HTTP headers");
                    let content_length = headers
                        .lines()
                        .filter_map(|line| line.split_once(':'))
                        .find(|(name, _)| name.eq_ignore_ascii_case("content-length"))
                        .map(|(_, value)| value.trim().parse::<usize>().expect("content length"))
                        .unwrap_or(0);
                    if request.len() >= headers_end + 4 + content_length {
                        break;
                    }
                }
            }
            started_tx.send(()).expect("signal refresh started");
            release_rx.await.expect("release delayed token response");
            let body = format!(
                r#"{{"access_token":"{access_token}","refresh_token":"rotated-refresh","token_type":"Bearer","expires_in":3600}}"#
            );
            let response = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
            // Revocation may already have closed the client; either transport outcome is valid.
            let _ = stream.write_all(response.as_bytes()).await;
        });
        Self {
            adapter,
            started,
            release,
            server,
        }
    }

    async fn wait_started(&mut self) {
        tokio::time::timeout(std::time::Duration::from_secs(2), &mut self.started)
            .await
            .expect("refresh must reach the gated server")
            .expect("refresh server started");
    }

    async fn release(self) {
        self.release.send(()).expect("release token response");
        self.server.await.expect("finish token fixture");
    }
}
