use super::test_support::{env_lock, TestEnv};
use super::*;
use rusqlite::Connection;

fn credential() -> OAuthCredential {
    OAuthCredential {
        provider: "Codex".into(),
        access_token: "access-canary".into(),
        refresh_token: Some("refresh-canary".into()),
        expires_at: chrono::Utc::now(),
        account_id: Some("account-canary".into()),
        ownership: OAuthCredentialOwnership::Tcui,
        source: OAuthCredentialSource::NativeOAuth,
    }
}

#[test]
fn oauth_uses_same_vault_and_plaintext_preference() {
    let _guard = env_lock().lock().expect("env lock");
    let env = TestEnv::new("oauth-vault");
    let config = env.initialized_config();
    let expected = credential();
    KeyStore::upsert_oauth(&config, &expected).expect("persist");
    KeyStore::set(&config, "OpenAI", "api-canary").expect("save API key");
    let bytes = std::fs::read(env.database_path()).expect("database");
    let raw = String::from_utf8_lossy(&bytes);
    assert!(
        !raw.contains("access-canary")
            && !raw.contains("refresh-canary")
            && !raw.contains("account-canary")
            && !raw.contains("api-canary")
    );
    KeyStore::lock_vault(&config).expect("lock");
    assert!(KeyStore::get_oauth(&config, "Codex").is_err());
    assert!(KeyStore::upsert_oauth(&config, &expected).is_err());
    assert!(KeyStore::remove_oauth(&config, "Codex").is_err());
    KeyStore::unlock_vault(&config, "test-only-vault-password").expect("unlock");
    assert_eq!(
        KeyStore::get_oauth(&config, "Codex").expect("read"),
        Some(expected.clone())
    );
    KeyStore::set_storage_preference(&config, CredentialStorage::Plaintext)
        .expect("select plaintext");
    KeyStore::lock_vault(&config).expect("lock");
    assert_eq!(
        KeyStore::get_oauth(&config, "Codex").expect("plaintext read"),
        Some(expected)
    );
    assert!(KeyStore::remove_oauth(&config, "Codex").expect("plaintext remove"));
}

#[test]
fn ciphertext_nonce_provider_and_label_tampering_fail_closed() {
    let _guard = env_lock().lock().expect("env lock");
    let env = TestEnv::new("vault-tamper");
    let config = env.initialized_config();
    KeyStore::upsert_oauth(&config, &credential()).expect("persist");
    let conn = Connection::open(env.database_path()).expect("database");
    conn.execute(
        "UPDATE tcui_credentials SET label='modified' WHERE kind='oauth'",
        [],
    )
    .expect("tamper label");
    assert!(matches!(
        KeyStore::get_oauth(&config, "Codex"),
        Err(KeyStoreError::VaultAuthentication)
    ));
    conn.execute(
        "UPDATE tcui_credentials SET label='Codex',id='Other' WHERE kind='oauth'",
        [],
    )
    .expect("tamper provider");
    assert!(KeyStore::get_oauth(&config, "Other").is_err());
    conn.execute(
        "UPDATE tcui_credentials SET id='Codex',nonce=x'00' WHERE kind='oauth'",
        [],
    )
    .expect("tamper nonce");
    assert!(KeyStore::get_oauth(&config, "Codex").is_err());
    conn.execute(
        "UPDATE tcui_credentials SET nonce=zeroblob(12),payload=zeroblob(32) WHERE kind='oauth'",
        [],
    )
    .expect("tamper payload");
    let error = KeyStore::get_oauth(&config, "Codex").expect_err("reject ciphertext");
    assert!(!format!("{error:?} {error}").contains("canary"));
}

#[test]
fn modified_verifier_revokes_previous_unlock_and_fails_closed() {
    let _guard = env_lock().lock().expect("env lock");
    let env = TestEnv::new("vault-verifier-tamper");
    let config = env.initialized_config();
    KeyStore::set(&config, "OpenAI", "api-canary").expect("persist");
    let conn = Connection::open(env.database_path()).expect("database");
    conn.execute("UPDATE tcui_credential_vault SET verifier=zeroblob(32)", [])
        .expect("tamper verifier");
    assert!(KeyStore::unlock_vault(&config, "test-only-vault-password").is_err());
    assert!(!KeyStore::vault_status(&config).expect("status").unlocked);
    assert!(KeyStore::get(&config, "OpenAI").is_err());
}

#[test]
fn wal_plaintext_is_removed_when_converted_to_encrypted_storage() {
    let _guard = env_lock().lock().expect("env lock");
    let env = TestEnv::new("vault-wal-cleanup");
    let config = env.initialized_config();
    KeyStore::set_storage_preference(&config, CredentialStorage::Plaintext).expect("plaintext");
    let conn = Connection::open(env.database_path()).expect("database");
    conn.execute_batch("PRAGMA journal_mode=WAL")
        .expect("WAL mode");
    KeyStore::set(&config, "OpenAI", "wal-plaintext-canary").expect("plaintext write");
    drop(conn);
    KeyStore::set_storage_preference(&config, CredentialStorage::Encrypted).expect("convert");
    for path in [
        env.database_path(),
        env.database_path().with_file_name("tcui.db-wal"),
        env.database_path().with_file_name("tcui.db-journal"),
    ] {
        if let Ok(bytes) = std::fs::read(path) {
            assert!(!String::from_utf8_lossy(&bytes).contains("wal-plaintext-canary"));
        }
    }
}

#[test]
fn incomplete_or_malformed_initialization_metadata_never_allows_reinitialization() {
    let _guard = env_lock().lock().expect("env lock");
    let env = TestEnv::new("vault-invalid-initialization");
    let config = env.initialized_config();
    KeyStore::set(&config, "OpenAI", "existing-canary").expect("persist");
    let conn = Connection::open(env.database_path()).expect("database");
    let (salt, nonce, verifier): (Vec<u8>, Vec<u8>, Vec<u8>) = conn
        .query_row(
            "SELECT salt,verifier_nonce,verifier FROM tcui_credential_vault",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .expect("original metadata");
    for corruption in [
        "salt=NULL",
        "verifier_nonce=NULL",
        "verifier=NULL",
        "salt=zeroblob(15)",
        "verifier_nonce=zeroblob(11)",
        "verifier=zeroblob(31)",
        "salt=NULL,verifier_nonce=NULL,verifier=NULL",
    ] {
        conn.execute(
            "UPDATE tcui_credential_vault SET salt=?1,verifier_nonce=?2,verifier=?3",
            rusqlite::params![salt, nonce, verifier],
        )
        .expect("restore metadata");
        conn.execute_batch(&format!("UPDATE tcui_credential_vault SET {corruption}"))
            .expect("corrupt metadata");
        assert!(
            matches!(
                KeyStore::vault_status(&config),
                Err(KeyStoreError::VaultAuthentication)
            ),
            "{corruption}"
        );
        assert!(KeyStore::get(&config, "OpenAI").is_err(), "{corruption}");
        assert!(
            KeyStore::set(&config, "OpenAI", "replacement").is_err(),
            "{corruption}"
        );
        assert!(
            KeyStore::initialize_vault(&config, "replacement-password").is_err(),
            "{corruption}"
        );
    }
}
