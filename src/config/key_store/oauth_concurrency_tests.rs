use super::test_support::{env_lock, TestEnv};
use super::*;

#[test]
fn concurrent_oauth_upserts_preserve_every_record() {
    let _guard = env_lock().lock().expect("env lock");
    let env = TestEnv::new("vault-concurrent");
    let config = env.initialized_config();
    let barrier = std::sync::Arc::new(std::sync::Barrier::new(16));
    let workers: Vec<_> = (0..16)
        .map(|index| {
            let config = config.clone();
            let barrier = barrier.clone();
            std::thread::spawn(move || {
                let credential = OAuthCredential {
                    provider: format!("Provider-{index}"),
                    access_token: format!("access-{index}"),
                    refresh_token: None,
                    expires_at: chrono::Utc::now(),
                    account_id: None,
                    ownership: OAuthCredentialOwnership::Tcui,
                    source: OAuthCredentialSource::NativeOAuth,
                };
                barrier.wait();
                KeyStore::upsert_oauth(&config, &credential).expect("write");
                credential
            })
        })
        .collect();
    for worker in workers {
        let expected = worker.join().expect("worker");
        assert_eq!(
            KeyStore::get_oauth(&config, &expected.provider).expect("read"),
            Some(expected)
        );
    }
}

fn oauth_fixture(provider: &str, token: &str) -> OAuthCredential {
    OAuthCredential {
        provider: provider.into(),
        access_token: token.into(),
        refresh_token: Some(format!("refresh-{token}")),
        expires_at: chrono::Utc::now(),
        account_id: Some("fixture-account".into()),
        ownership: OAuthCredentialOwnership::Tcui,
        source: OAuthCredentialSource::NativeOAuth,
    }
}

fn persist_login(
    config: &AppConfig,
    provider: &str,
    token: &str,
    storage: CredentialStorage,
) -> std::result::Result<(), KeyStoreError> {
    if provider == "Codex" {
        KeyStore::upsert_oauth_with_storage(config, &oauth_fixture(provider, token), storage)
    } else {
        let credential = Credential::ApiKey(
            ApiKeyCredential::new(
                provider,
                token,
                ApiKeyCredentialOwnership::Tcui,
                ApiKeyCredentialSource::OpenRouterPkce,
            )
            .expect("valid owned credential"),
        );
        KeyStore::upsert_credential_with_storage(config, &credential, storage)
    }
}

#[test]
fn pending_explicit_logins_reject_opposite_storage_collision_without_replacing_sibling() {
    let _guard = env_lock().lock().expect("env lock");
    for provider in ["Codex", "OpenRouter OAuth"] {
        for (pending, inserted) in [
            (CredentialStorage::Encrypted, CredentialStorage::Plaintext),
            (CredentialStorage::Plaintext, CredentialStorage::Encrypted),
        ] {
            let env = TestEnv::new("login-storage-collision");
            let config = env.initialized_config();
            let selected = KeyStore::connection_id(&config, provider, pending).expect("unused ID");
            assert!(!KeyStore::has_credential(&config, &selected).expect("absent"));
            let (complete, pending_login) = std::sync::mpsc::channel();
            let worker_config = config.clone();
            let worker = std::thread::spawn(move || {
                pending_login.recv().expect("authorization completed");
                persist_login(&worker_config, &selected, "pending-login-canary", pending)
            });

            persist_login(&config, provider, "original-login-canary", inserted)
                .expect("concurrent sibling wins");
            complete.send(()).expect("release pending login");
            assert!(matches!(
                worker.join().expect("pending login"),
                Err(KeyStoreError::InvalidCredentialPayload)
            ));
            assert_eq!(
                KeyStore::provider_storage(&config, provider).expect("storage"),
                Some(inserted)
            );
            let token = if provider == "Codex" {
                KeyStore::get_oauth(&config, provider)
                    .expect("original OAuth")
                    .expect("present")
                    .access_token
            } else {
                KeyStore::get_api_key_credential(&config, provider)
                    .expect("original owned key")
                    .expect("present")
                    .api_key()
                    .to_owned()
            };
            assert_eq!(token, "original-login-canary");
            assert!(!std::fs::read(env.database_path())
                .expect("database")
                .windows(b"pending-login-canary".len())
                .any(|bytes| bytes == b"pending-login-canary"));
        }
    }
}

#[test]
fn conditional_oauth_refresh_rejects_removed_replaced_and_invalidated_records() {
    let _guard = env_lock().lock().expect("env lock");
    let env = TestEnv::new("oauth-refresh-cas");
    let config = env.initialized_config();
    let expected = oauth_fixture("Codex", "expected");
    let replacement = oauth_fixture("Codex", "refreshed");
    KeyStore::upsert_oauth(&config, &expected).expect("original");
    assert!(
        !KeyStore::replace_oauth_if_current(&config, &expected, &replacement, || false)
            .expect("invalidated refresh")
    );
    assert_eq!(
        KeyStore::get_oauth(&config, "Codex").expect("preserved"),
        Some(expected.clone())
    );
    KeyStore::remove_oauth(&config, "Codex").expect("remove");
    assert!(
        !KeyStore::replace_oauth_if_current(&config, &expected, &replacement, || true)
            .expect("removed refresh")
    );
    assert!(KeyStore::get_oauth(&config, "Codex")
        .expect("not resurrected")
        .is_none());
    let reconnected = oauth_fixture("Codex", "reconnected");
    KeyStore::upsert_oauth(&config, &reconnected).expect("reconnect");
    assert!(
        !KeyStore::replace_oauth_if_current(&config, &expected, &replacement, || true)
            .expect("replaced refresh")
    );
    assert_eq!(
        KeyStore::get_oauth(&config, "Codex").expect("preserved"),
        Some(reconnected)
    );
}

#[test]
fn conditional_oauth_refresh_preserves_existing_storage_despite_default_changes() {
    let _guard = env_lock().lock().expect("env lock");
    for storage in [CredentialStorage::Encrypted, CredentialStorage::Plaintext] {
        let env = TestEnv::new("oauth-refresh-storage");
        let config = env.initialized_config();
        let expected = oauth_fixture("Codex", "expected");
        let replacement = oauth_fixture("Codex", "refreshed");
        KeyStore::upsert_oauth_with_storage(&config, &expected, storage).expect("original");
        let other = if storage == CredentialStorage::Encrypted {
            CredentialStorage::Plaintext
        } else {
            CredentialStorage::Encrypted
        };
        KeyStore::set_storage_default(&config, other).expect("changed default");
        assert!(
            KeyStore::replace_oauth_if_current(&config, &expected, &replacement, || true)
                .expect("current refresh")
        );
        assert_eq!(
            KeyStore::provider_storage(&config, "Codex").expect("storage"),
            Some(storage)
        );
        assert_eq!(
            KeyStore::get_oauth(&config, "Codex").expect("refreshed"),
            Some(replacement)
        );
    }
}
