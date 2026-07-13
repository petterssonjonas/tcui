use super::test_support::{TestEnv, env_lock};
use super::*;
use chrono::{DateTime, Utc};
use std::path::PathBuf;

fn native_credential() -> OAuthCredential {
    OAuthCredential {
        provider: "codex".to_string(),
        access_token: "access-canary".to_string(),
        refresh_token: Some("refresh-canary".to_string()),
        expires_at: DateTime::parse_from_rfc3339("2030-01-02T03:04:05Z")
            .expect("valid expiry fixture")
            .with_timezone(&Utc),
        account_id: Some("account-canary".to_string()),
        ownership: OAuthCredentialOwnership::Tcui,
        source: OAuthCredentialSource::NativeOAuth,
    }
}

fn write_future_fixture(config: &AppConfig) -> (PathBuf, String) {
    let encrypted = crate::storage::Storage::encrypt_shared_text("legacy-api-key")
        .expect("encrypt legacy API key");
    let fixture = format!(
        "version = 99\n[keys]\nOpenAI = \"{encrypted}\"\n[oauth]\nfuture = {{ record = \"opaque\" }}\n"
    );
    let path = PathBuf::from(config.key_file.as_deref().expect("key file path"));
    std::fs::create_dir_all(path.parent().expect("key file parent"))
        .expect("create key file parent");
    std::fs::write(&path, &fixture).expect("write future-version fixture");
    (path, fixture)
}

#[test]
fn upsert_oauth_rejects_future_store_without_touching_unknown_oauth_bytes() {
    let _guard = env_lock().lock().expect("env lock poisoned");
    let env = TestEnv::new("oauth-future-version-upsert");
    let config = env.config();
    let (path, fixture) = write_future_fixture(&config);

    let error = KeyStore::upsert_oauth(&config, &native_credential())
        .expect_err("future OAuth store must not be rewritten");

    assert!(matches!(
        error,
        KeyStoreError::UnsupportedVersion { version: 99 }
    ));
    assert_eq!(
        std::fs::read_to_string(path).expect("read future-version fixture"),
        fixture
    );
}

#[test]
fn remove_oauth_rejects_future_store_without_touching_unknown_oauth_bytes() {
    let _guard = env_lock().lock().expect("env lock poisoned");
    let env = TestEnv::new("oauth-future-version-remove");
    let config = env.config();
    let (path, fixture) = write_future_fixture(&config);

    let error = KeyStore::remove_oauth(&config, "codex")
        .expect_err("future OAuth store must not be rewritten");

    assert!(matches!(
        error,
        KeyStoreError::UnsupportedVersion { version: 99 }
    ));
    assert_eq!(
        std::fs::read_to_string(path).expect("read future-version fixture"),
        fixture
    );
}
