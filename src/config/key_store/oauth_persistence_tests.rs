use super::persistence::write_file;
use super::test_support::{TestEnv, env_lock};
use super::*;
use chrono::{DateTime, Utc};
use std::collections::BTreeMap;
use std::path::PathBuf;

const OAUTH_RECORD_KIND: &str = "oauth:codex";

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

fn key_path(config: &AppConfig) -> PathBuf {
    PathBuf::from(config.key_file.as_deref().expect("key file path"))
}

#[test]
fn oauth_toml_contains_only_ciphertext_and_preserves_legacy_key() {
    let _guard = env_lock().lock().expect("env lock poisoned");
    let env = TestEnv::new("oauth-ciphertext-only");
    let config = env.config();
    let credential = native_credential("access-canary", Some("refresh-canary"));

    KeyStore::save_keys(
        &config,
        &[("OpenAI".to_string(), "legacy-api-key".to_string())],
    )
    .expect("persist legacy API key");
    KeyStore::upsert_oauth(&config, &credential).expect("persist OAuth credential");
    let raw = std::fs::read_to_string(key_path(&config)).expect("read credential TOML");
    let legacy = KeyStore::get(&config, "OpenAI").expect("read legacy API key");

    assert!(raw.contains("enc:v1:"));
    assert!(!raw.contains("access-canary"));
    assert!(!raw.contains("refresh-canary"));
    assert!(!raw.contains("account-canary"));
    assert_eq!(legacy.as_deref(), Some("legacy-api-key"));
}

#[test]
fn save_keys_replaces_legacy_keys_without_removing_owned_oauth_record() {
    let _guard = env_lock().lock().expect("env lock poisoned");
    let env = TestEnv::new("oauth-preserved-by-legacy-save");
    let config = env.config();
    let credential = native_credential("access-canary", Some("refresh-canary"));

    KeyStore::upsert_oauth(&config, &credential).expect("persist OAuth credential");
    KeyStore::save_keys(
        &config,
        &[("OpenAI".to_string(), "legacy-api-key".to_string())],
    )
    .expect("replace legacy key set");
    let loaded = KeyStore::get_oauth(&config, "codex").expect("read preserved OAuth credential");

    assert_eq!(loaded, Some(credential));
}

#[test]
fn corrupt_oauth_ciphertext_returns_typed_error_without_damaging_legacy_key() {
    let _guard = env_lock().lock().expect("env lock poisoned");
    let env = TestEnv::new("oauth-corrupt-ciphertext");
    let config = env.config();
    let credential = native_credential("access-canary", Some("refresh-canary"));

    KeyStore::save_keys(
        &config,
        &[("OpenAI".to_string(), "legacy-api-key".to_string())],
    )
    .expect("persist legacy API key");
    KeyStore::upsert_oauth(&config, &credential).expect("persist OAuth credential");
    std::fs::write(
        key_path(&config),
        "version = 1\n[keys]\nOpenAI = \"legacy-api-key\"\n[oauth]\ncodex = \"enc:v1:not-base64\"\n",
    )
    .expect("corrupt OAuth ciphertext");

    let error = KeyStore::get_oauth(&config, "codex").expect_err("corrupt OAuth must fail");
    let legacy = KeyStore::get(&config, "OpenAI").expect("read legacy API key");

    assert!(matches!(error, KeyStoreError::OauthDecrypt));
    assert_eq!(legacy.as_deref(), Some("legacy-api-key"));
}

#[test]
fn oauth_errors_redact_secret_canaries_in_display_and_debug() {
    let _guard = env_lock().lock().expect("env lock poisoned");
    let env = TestEnv::new("oauth-error-redaction");
    let config = env.config();
    std::fs::create_dir_all(key_path(&config).parent().expect("key file parent"))
        .expect("create key file parent");
    std::fs::write(
        key_path(&config),
        "version = 1\n[oauth]\ncodex = \"access-canary-refresh-canary\"\n",
    )
    .expect("write invalid OAuth fixture");

    let error = KeyStore::get_oauth(&config, "codex").expect_err("invalid OAuth must fail");
    let display = error.to_string();
    let debug = format!("{error:?}");

    for secret in ["access-canary", "refresh-canary"] {
        assert!(!display.contains(secret));
        assert!(!debug.contains(secret));
    }
}

#[test]
fn oauth_lifecycle_uses_real_store_without_serializing_secrets_into_config() {
    let _guard = env_lock().lock().expect("env lock poisoned");
    let env = TestEnv::new("oauth-manual-lifecycle");
    let config = env.config();
    let credential = native_credential("access-canary", Some("refresh-canary"));

    KeyStore::save_keys(
        &config,
        &[("OpenAI".to_string(), "legacy-api-key".to_string())],
    )
    .expect("persist legacy API key");
    KeyStore::upsert_oauth(&config, &credential).expect("persist OAuth credential");
    let raw = std::fs::read_to_string(key_path(&config)).expect("inspect credential TOML");
    let loaded = KeyStore::get_oauth(&config, "codex").expect("read OAuth credential");
    let removed = KeyStore::remove_oauth(&config, "codex").expect("remove OAuth credential");
    let absent = KeyStore::get_oauth(&config, "codex").expect("read removed OAuth credential");
    let legacy = KeyStore::get(&config, "OpenAI").expect("read retained legacy API key");
    let serialized_config = toml::to_string_pretty(&config).expect("serialize app config");

    assert!(!raw.contains("access-canary"));
    assert!(!raw.contains("refresh-canary"));
    assert_eq!(loaded, Some(credential));
    assert!(removed);
    assert_eq!(absent, None);
    assert_eq!(legacy.as_deref(), Some("legacy-api-key"));
    assert!(!serialized_config.contains("access-canary"));
}

#[test]
fn oauth_store_rejects_unknown_version_with_typed_error() {
    let _guard = env_lock().lock().expect("env lock poisoned");
    let env = TestEnv::new("oauth-unknown-version");
    let config = env.config();
    std::fs::create_dir_all(key_path(&config).parent().expect("key file parent"))
        .expect("create key file parent");
    std::fs::write(key_path(&config), "version = 99\n[oauth]\n")
        .expect("write unknown version fixture");

    let error = KeyStore::get_oauth(&config, "codex").expect_err("unknown version must fail");

    assert!(matches!(
        error,
        KeyStoreError::UnsupportedVersion { version: 99 }
    ));
}

#[test]
fn oauth_store_rejects_authenticated_payload_with_missing_required_fields() {
    let _guard = env_lock().lock().expect("env lock poisoned");
    let env = TestEnv::new("oauth-invalid-payload");
    let config = env.config();
    let shared_key = crate::storage::crypto::SharedKey::load_or_create_default(
        &crate::storage::paths::TcuiDataPaths::discover(),
    )
    .expect("load shared key");
    let ciphertext = crate::storage::crypto::encrypt_serialized(
        &shared_key.key,
        OAUTH_RECORD_KIND,
        &serde_json::json!({"provider": "codex"}),
    )
    .expect("encrypt malformed OAuth payload");
    std::fs::create_dir_all(key_path(&config).parent().expect("key file parent"))
        .expect("create key file parent");
    std::fs::write(
        key_path(&config),
        format!("version = 1\n[oauth]\ncodex = \"{ciphertext}\"\n"),
    )
    .expect("write malformed OAuth fixture");

    let error = KeyStore::get_oauth(&config, "codex").expect_err("invalid OAuth payload must fail");

    assert!(matches!(error, KeyStoreError::InvalidOauthPayload));
}

#[test]
fn failed_rename_cleans_temporary_file_when_destination_is_a_directory() {
    let _guard = env_lock().lock().expect("env lock poisoned");
    let env = TestEnv::new("oauth-rename-failure");
    let config = env.config();
    let path = key_path(&config);
    std::fs::create_dir_all(&path).expect("create directory at key file path");
    let replacement = StoredKeysFile {
        version: 1,
        keys: BTreeMap::new(),
        oauth: BTreeMap::new(),
        credentials: BTreeMap::new(),
    };

    let error = write_file(&path, &replacement).expect_err("directory destination must fail");

    assert!(matches!(error, KeyStoreError::Write));
    assert!(
        std::fs::read_dir(path.parent().expect("key file parent"))
            .expect("read key file parent")
            .all(|entry| {
                !entry
                    .expect("read directory entry")
                    .file_name()
                    .to_string_lossy()
                    .starts_with(".tcui-keys-")
            }),
        "failed rename must remove the secret-bearing temporary file"
    );
}

#[cfg(unix)]
#[test]
fn upsert_oauth_sets_owner_read_write_permissions() {
    use std::os::unix::fs::PermissionsExt;

    let _guard = env_lock().lock().expect("env lock poisoned");
    let env = TestEnv::new("oauth-permissions");
    let config = env.config();
    let credential = native_credential("access-canary", None);

    KeyStore::upsert_oauth(&config, &credential).expect("persist OAuth credential");

    let mode = std::fs::metadata(key_path(&config))
        .expect("credential file metadata")
        .permissions()
        .mode()
        & 0o777;
    assert_eq!(mode, 0o600);
}
