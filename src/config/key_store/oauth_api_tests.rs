use super::test_support::{env_lock, TestEnv};
use super::*;
use chrono::{DateTime, Utc};

fn native_credential(access_token: &str, refresh_token: Option<&str>) -> OAuthCredential {
    OAuthCredential {
        provider: "codex".to_string(),
        access_token: access_token.to_string(),
        refresh_token: refresh_token.map(str::to_string),
        expires_at: DateTime::parse_from_rfc3339("2030-01-02T03:04:05Z")
            .expect("valid expiry fixture")
            .with_timezone(&Utc),
        account_id: Some("account-canary".to_string()),
        ownership: OAuthCredentialOwnership::Tcui,
        source: OAuthCredentialSource::NativeOAuth,
    }
}

#[test]
fn oauth_round_trip_returns_exact_owned_credential() {
    let _guard = env_lock().lock().expect("env lock poisoned");
    let env = TestEnv::new("oauth-round-trip");
    let config = env.config();
    let credential = native_credential("access-canary", Some("refresh-canary"));

    KeyStore::upsert_oauth(&config, &credential).expect("persist OAuth credential");
    let loaded = KeyStore::get_oauth(&config, "codex").expect("read OAuth credential");

    assert_eq!(loaded, Some(credential));
}

#[test]
fn upsert_oauth_replaces_prior_record_for_provider() {
    let _guard = env_lock().lock().expect("env lock poisoned");
    let env = TestEnv::new("oauth-replace");
    let config = env.config();
    let first = native_credential("first-access", Some("first-refresh"));
    let replacement = native_credential("second-access", Some("second-refresh"));

    KeyStore::upsert_oauth(&config, &first).expect("persist first OAuth credential");
    KeyStore::upsert_oauth(&config, &replacement).expect("replace OAuth credential");
    let loaded = KeyStore::get_oauth(&config, "codex").expect("read replacement OAuth credential");

    assert_eq!(loaded, Some(replacement));
}

#[test]
fn remove_oauth_is_idempotent_after_record_is_deleted() {
    let _guard = env_lock().lock().expect("env lock poisoned");
    let env = TestEnv::new("oauth-remove");
    let config = env.config();
    let credential = native_credential("access-canary", None);

    KeyStore::upsert_oauth(&config, &credential).expect("persist OAuth credential");
    assert!(KeyStore::remove_oauth(&config, "codex").expect("remove OAuth credential"));
    let removed_again =
        KeyStore::remove_oauth(&config, "codex").expect("repeat OAuth credential removal");

    assert!(!removed_again);
}

#[test]
fn oauth_credential_debug_redacts_access_and_refresh_tokens() {
    let credential = native_credential("access-canary", Some("refresh-canary"));

    let debug = format!("{credential:?}");

    assert!(!debug.contains("access-canary"));
    assert!(!debug.contains("refresh-canary"));
    assert!(!debug.contains("account-canary"));
}
