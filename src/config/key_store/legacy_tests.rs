use super::test_support::{env_lock, TestEnv};
use super::*;
use std::path::PathBuf;

#[test]
fn stores_keys_encrypted_with_shared_key() {
    let _guard = env_lock().lock().expect("env lock poisoned");
    let env = TestEnv::new("keys-encryption");
    let config = env.config();

    KeyStore::save_keys(&config, &[("OpenAI".to_string(), "sk-secret".to_string())])
        .expect("save encrypted keys");
    let raw = std::fs::read_to_string(config.key_file.as_ref().expect("key file path"))
        .expect("read raw key file");
    assert!(raw.contains("enc:v1:"));
    assert!(!raw.contains("sk-secret"));

    let key = KeyStore::get(&config, "OpenAI").expect("load decrypted key");
    assert_eq!(key.as_deref(), Some("sk-secret"));
}

#[test]
fn default_key_path_uses_xdg_data_home() {
    let _guard = env_lock().lock().expect("env lock poisoned");
    let env = TestEnv::new("keys-default-path");
    let config = AppConfig::default();

    KeyStore::save_keys(&config, &[("OpenAI".to_string(), "sk-secret".to_string())])
        .expect("save encrypted keys");

    let key_path = env.data_home().join("tcui").join("keys.toml");
    let raw = std::fs::read_to_string(&key_path).expect("read raw key file");
    assert!(raw.contains("enc:v1:"));
    assert!(!raw.contains("sk-secret"));
}

#[test]
fn get_loads_legacy_keys_table_without_rewriting_file() {
    let _guard = env_lock().lock().expect("env lock poisoned");
    let env = TestEnv::new("keys-legacy-fixture");
    let config = env.config();
    let encrypted = crate::storage::Storage::encrypt_shared_text("legacy-api-key")
        .expect("encrypt legacy API key");
    let fixture = format!("[keys]\nOpenAI = \"{encrypted}\"\n");
    let key_path = PathBuf::from(config.key_file.as_ref().expect("key file path"));
    std::fs::create_dir_all(key_path.parent().expect("key file parent"))
        .expect("create key file parent");
    std::fs::write(&key_path, &fixture).expect("write legacy key fixture");

    let key = KeyStore::get(&config, "OpenAI").expect("read legacy API key");

    assert_eq!(key.as_deref(), Some("legacy-api-key"));
    assert_eq!(
        std::fs::read_to_string(&key_path).expect("read legacy fixture after get"),
        fixture
    );
}

#[test]
fn get_reads_legacy_key_when_future_store_version_has_oauth_data() {
    let _guard = env_lock().lock().expect("env lock poisoned");
    let env = TestEnv::new("keys-future-version-legacy-read");
    let config = env.config();
    let encrypted = crate::storage::Storage::encrypt_shared_text("legacy-api-key")
        .expect("encrypt legacy API key");
    let fixture = format!(
        "version = 99\n[keys]\nOpenAI = \"{encrypted}\"\n[oauth]\nfuture = {{ record = \"opaque\" }}\n"
    );
    let key_path = PathBuf::from(config.key_file.as_ref().expect("key file path"));
    std::fs::create_dir_all(key_path.parent().expect("key file parent"))
        .expect("create key file parent");
    std::fs::write(&key_path, fixture).expect("write future-version key fixture");

    let key = KeyStore::get(&config, "OpenAI").expect("read legacy API key");

    assert_eq!(key.as_deref(), Some("legacy-api-key"));
}

#[test]
fn oauth_api_rejects_future_store_version_without_interpreting_oauth_records() {
    let _guard = env_lock().lock().expect("env lock poisoned");
    let env = TestEnv::new("keys-future-version-oauth-read");
    let config = env.config();
    let encrypted = crate::storage::Storage::encrypt_shared_text("legacy-api-key")
        .expect("encrypt legacy API key");
    let fixture = format!(
        "version = 99\n[keys]\nOpenAI = \"{encrypted}\"\n[oauth]\nfuture = {{ record = \"opaque\" }}\n"
    );
    let key_path = PathBuf::from(config.key_file.as_ref().expect("key file path"));
    std::fs::create_dir_all(key_path.parent().expect("key file parent"))
        .expect("create key file parent");
    std::fs::write(&key_path, fixture).expect("write future-version key fixture");

    let error = KeyStore::get_oauth(&config, "future").expect_err("future OAuth must fail");

    assert!(matches!(
        error,
        KeyStoreError::UnsupportedVersion { version: 99 }
    ));
}

#[test]
fn save_keys_rejects_future_store_without_touching_unknown_oauth_bytes() {
    let _guard = env_lock().lock().expect("env lock poisoned");
    let env = TestEnv::new("keys-future-version-legacy-save");
    let config = env.config();
    let encrypted = crate::storage::Storage::encrypt_shared_text("legacy-api-key")
        .expect("encrypt legacy API key");
    let fixture = format!(
        "version = 99\n[keys]\nOpenAI = \"{encrypted}\"\n[oauth]\nfuture = {{ record = \"opaque\" }}\n"
    );
    let key_path = PathBuf::from(config.key_file.as_ref().expect("key file path"));
    std::fs::create_dir_all(key_path.parent().expect("key file parent"))
        .expect("create key file parent");
    std::fs::write(&key_path, &fixture).expect("write future-version key fixture");

    let error = KeyStore::save_keys(
        &config,
        &[("OpenAI".to_string(), "replacement".to_string())],
    )
    .expect_err("future store must not be rewritten");

    assert!(matches!(
        error.downcast_ref::<KeyStoreError>(),
        Some(KeyStoreError::UnsupportedVersion { version: 99 })
    ));
    assert_eq!(
        std::fs::read_to_string(key_path).expect("read future-version fixture"),
        fixture
    );
}

#[test]
fn get_propagates_decryption_error_for_malformed_legacy_ciphertext() {
    let _guard = env_lock().lock().expect("env lock poisoned");
    let env = TestEnv::new("keys-decryption-error");
    let config = env.config();
    let key_path = PathBuf::from(config.key_file.as_ref().expect("key file path"));
    std::fs::create_dir_all(key_path.parent().expect("key file parent"))
        .expect("create key file parent");
    std::fs::write(&key_path, "[keys]\nOpenAI = \"enc:v1:not-base64\"\n")
        .expect("write malformed key fixture");

    let result = KeyStore::get(&config, "OpenAI");

    assert!(result.is_err(), "malformed ciphertext must fail to decrypt");
}

#[test]
fn save_keys_replaces_the_legacy_key_set() {
    let _guard = env_lock().lock().expect("env lock poisoned");
    let env = TestEnv::new("keys-replace-legacy-set");
    let config = env.config();

    KeyStore::save_keys(
        &config,
        &[
            ("OpenAI".to_string(), "first-api-key".to_string()),
            ("Groq".to_string(), "second-api-key".to_string()),
        ],
    )
    .expect("save initial legacy keys");
    KeyStore::save_keys(
        &config,
        &[("Mistral".to_string(), "replacement-api-key".to_string())],
    )
    .expect("replace legacy keys");

    let removed = KeyStore::get(&config, "OpenAI").expect("read replaced legacy key");
    let replacement = KeyStore::get(&config, "Mistral").expect("read replacement legacy key");

    assert_eq!(removed, None);
    assert_eq!(replacement.as_deref(), Some("replacement-api-key"));
}

#[cfg(unix)]
#[test]
fn save_keys_sets_owner_read_write_permissions() {
    use std::os::unix::fs::PermissionsExt;

    let _guard = env_lock().lock().expect("env lock poisoned");
    let env = TestEnv::new("keys-permissions");
    let config = env.config();

    KeyStore::save_keys(&config, &[("OpenAI".to_string(), "sk-secret".to_string())])
        .expect("save encrypted keys");

    let mode = std::fs::metadata(config.key_file.as_ref().expect("key file path"))
        .expect("key file metadata")
        .permissions()
        .mode()
        & 0o777;
    assert_eq!(mode, 0o600);
}

#[cfg(unix)]
#[test]
fn get_rejects_symlink_key_file_without_reading_its_target() {
    use std::os::unix::fs::symlink;

    let _guard = env_lock().lock().expect("env lock poisoned");
    let env = TestEnv::new("keys-symlink-read");
    let config = env.config();
    let encrypted = crate::storage::Storage::encrypt_shared_text("legacy-api-key")
        .expect("encrypt legacy API key");
    let fixture = format!("[keys]\nOpenAI = \"{encrypted}\"\n");
    let key_path = PathBuf::from(config.key_file.as_ref().expect("key file path"));
    let target = key_path.with_file_name("symlink-target.toml");
    std::fs::create_dir_all(key_path.parent().expect("key file parent"))
        .expect("create key file parent");
    std::fs::write(&target, &fixture).expect("write symlink target");
    symlink(&target, &key_path).expect("create key-file symlink");

    let error = KeyStore::get(&config, "OpenAI").expect_err("symlink key file must fail");

    assert!(error.downcast_ref::<KeyStoreError>().is_some());
    assert_eq!(
        std::fs::read_to_string(target).expect("read symlink target"),
        fixture
    );
}

#[cfg(unix)]
#[test]
fn save_keys_rejects_symlink_key_file_without_clobbering_its_target() {
    use std::os::unix::fs::symlink;

    let _guard = env_lock().lock().expect("env lock poisoned");
    let env = TestEnv::new("keys-symlink-save");
    let config = env.config();
    let encrypted = crate::storage::Storage::encrypt_shared_text("legacy-api-key")
        .expect("encrypt legacy API key");
    let fixture = format!("[keys]\nOpenAI = \"{encrypted}\"\n");
    let key_path = PathBuf::from(config.key_file.as_ref().expect("key file path"));
    let target = key_path.with_file_name("symlink-target.toml");
    std::fs::create_dir_all(key_path.parent().expect("key file parent"))
        .expect("create key file parent");
    std::fs::write(&target, &fixture).expect("write symlink target");
    symlink(&target, &key_path).expect("create key-file symlink");

    let error = KeyStore::save_keys(
        &config,
        &[("OpenAI".to_string(), "replacement".to_string())],
    )
    .expect_err("symlink key file must fail");

    assert!(error.downcast_ref::<KeyStoreError>().is_some());
    assert_eq!(
        std::fs::read_to_string(target).expect("read symlink target"),
        fixture
    );
}
