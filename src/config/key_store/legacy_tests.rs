use super::test_support::{env_lock, TestEnv};
use super::*;

#[test]
fn encrypted_vault_reopens_only_with_correct_password() {
    let _guard = env_lock().lock().expect("env lock");
    let env = TestEnv::new("vault-reopen");
    let config = env.config();
    assert!(!KeyStore::vault_status(&config).expect("status").initialized);
    assert!(KeyStore::set(&config, "OpenAI", "secret-canary").is_err());
    KeyStore::initialize_vault(&config, "test-password").expect("initialize");
    KeyStore::set(&config, "OpenAI", "secret-canary").expect("save");
    KeyStore::lock_vault(&config).expect("lock");
    assert!(KeyStore::get(&config, "OpenAI").is_err());
    assert!(KeyStore::set(&config, "OpenAI", "replacement").is_err());
    assert!(KeyStore::remove(&config, "OpenAI").is_err());
    assert!(KeyStore::unlock_vault(&config, "wrong-password").is_err());
    assert!(!KeyStore::vault_status(&config).expect("status").unlocked);
    KeyStore::unlock_vault(&config, "test-password").expect("unlock");
    assert_eq!(
        KeyStore::get(&config, "OpenAI").expect("read").as_deref(),
        Some("secret-canary")
    );
    assert!(KeyStore::initialize_vault(&config, "new-password").is_err());
    assert!(!env.data_home().join("tcui/chat.key").exists());
    assert!(!std::path::Path::new(config.key_file.as_deref().expect("path")).exists());
    let bytes = std::fs::read(env.database_path()).expect("database");
    assert!(!String::from_utf8_lossy(&bytes).contains("secret-canary"));
}

#[test]
fn plaintext_requires_explicit_selection_and_works_while_locked() {
    let _guard = env_lock().lock().expect("env lock");
    let env = TestEnv::new("vault-plaintext");
    let config = env.config();
    KeyStore::set_storage_preference(&config, CredentialStorage::Plaintext)
        .expect("explicit plaintext");
    KeyStore::set(&config, "OpenAI", "plaintext-canary").expect("save plaintext");
    KeyStore::lock_vault(&config).expect("lock");
    assert_eq!(
        KeyStore::get(&config, "OpenAI").expect("read").as_deref(),
        Some("plaintext-canary")
    );
    assert!(
        String::from_utf8_lossy(&std::fs::read(env.database_path()).expect("database"))
            .contains("plaintext-canary")
    );
    assert!(KeyStore::set_secret(
        &config,
        "host",
        "Host",
        SecretKind::Password,
        "machine-secret"
    )
    .is_err());
    KeyStore::initialize_vault(&config, "test-password").expect("initialize");
    KeyStore::set_storage_preference(&config, CredentialStorage::Encrypted)
        .expect("encrypt existing");
    let bytes = std::fs::read(env.database_path()).expect("database");
    assert!(!String::from_utf8_lossy(&bytes).contains("plaintext-canary"));
    KeyStore::lock_vault(&config).expect("lock");
    assert!(KeyStore::get(&config, "OpenAI").is_err());
    assert!(KeyStore::set_storage_preference(&config, CredentialStorage::Plaintext).is_err());
    KeyStore::unlock_vault(&config, "test-password").expect("unlock");
    KeyStore::set_storage_preference(&config, CredentialStorage::Plaintext)
        .expect("decrypt existing");
    KeyStore::lock_vault(&config).expect("lock");
    assert_eq!(
        KeyStore::get(&config, "OpenAI").expect("read").as_deref(),
        Some("plaintext-canary")
    );
}

#[test]
fn machine_passwords_and_ssh_keys_are_encrypted_and_managed_by_id() {
    let _guard = env_lock().lock().expect("env lock");
    let env = TestEnv::new("vault-machine");
    let config = env.initialized_config();
    KeyStore::set_secret(
        &config,
        "host",
        "Production host",
        SecretKind::Password,
        "machine-canary",
    )
    .expect("password");
    KeyStore::set_secret(
        &config,
        "ssh",
        "Deploy key",
        SecretKind::SshKey,
        "ssh-key-canary",
    )
    .expect("ssh key");
    let entries = KeyStore::list_secrets(&config).expect("list");
    assert_eq!(entries.len(), 2);
    assert!(entries
        .iter()
        .any(|entry| entry.id == "ssh" && entry.kind == SecretKind::SshKey));
    assert_eq!(
        KeyStore::get_secret(&config, "host")
            .expect("read")
            .as_deref(),
        Some("machine-canary")
    );
    KeyStore::lock_vault(&config).expect("lock");
    assert!(KeyStore::list_secrets(&config).is_err());
    assert!(KeyStore::remove_secret(&config, "ssh").is_err());
    KeyStore::unlock_vault(&config, "test-only-vault-password").expect("unlock");
    assert!(KeyStore::remove_secret(&config, "ssh").expect("remove"));
    assert!(!KeyStore::remove_secret(&config, "ssh").expect("idempotent"));
    KeyStore::set_secret(
        &config,
        "host",
        "Replacement key",
        SecretKind::SshKey,
        "replacement-canary",
    )
    .expect("replace type");
    assert_eq!(KeyStore::list_secrets(&config).expect("list").len(), 1);
    let bytes = std::fs::read(env.database_path()).expect("database");
    let raw = String::from_utf8_lossy(&bytes);
    assert!(
        !raw.contains("machine-canary")
            && !raw.contains("ssh-key-canary")
            && !raw.contains("replacement-canary")
    );
}

#[test]
fn empty_password_never_initializes_vault() {
    let _guard = env_lock().lock().expect("env lock");
    let env = TestEnv::new("vault-empty-password");
    let config = env.config();
    assert!(matches!(
        KeyStore::initialize_vault(&config, ""),
        Err(KeyStoreError::EmptyPassword)
    ));
    assert!(!KeyStore::vault_status(&config).expect("status").initialized);
}

#[test]
fn failed_unlock_clears_an_existing_unlock_key() {
    let _guard = env_lock().lock().expect("env lock");
    let env = TestEnv::new("vault-relock-on-failure");
    let config = env.initialized_config();
    KeyStore::set(&config, "OpenAI", "api-canary").expect("persist");
    assert!(KeyStore::unlock_vault(&config, "wrong-password").is_err());
    assert!(!KeyStore::vault_status(&config).expect("status").unlocked);
    assert!(KeyStore::get(&config, "OpenAI").is_err());
}

#[test]
fn connection_defaults_never_convert_or_overwrite_other_records() {
    let _guard = env_lock().lock().expect("env lock");
    let env = TestEnv::new("mixed-connection-storage");
    let config = env.initialized_config();
    KeyStore::set(&config, "OpenAI", "encrypted-original").expect("encrypted connection");
    KeyStore::set_storage_default(&config, CredentialStorage::Plaintext).expect("new default");
    KeyStore::lock_vault(&config).expect("simulate restart");
    KeyStore::set_with_storage(
        &config,
        "OpenAI::plaintext",
        "plaintext-second",
        CredentialStorage::Plaintext,
    )
    .expect("separate plaintext connection");
    assert!(KeyStore::has_credential(&config, "OpenAI").expect("locked presence"));
    assert_eq!(
        KeyStore::provider_storage(&config, "OpenAI").expect("metadata"),
        Some(CredentialStorage::Encrypted)
    );
    assert_eq!(
        KeyStore::provider_storage(&config, "OpenAI::plaintext").expect("metadata"),
        Some(CredentialStorage::Plaintext)
    );
    assert!(KeyStore::get(&config, "OpenAI").is_err());
    assert!(KeyStore::set_with_storage(
        &config,
        "OpenAI",
        "replacement",
        CredentialStorage::Plaintext
    )
    .is_err());
    assert!(KeyStore::set(&config, "OpenAI", "replacement").is_err());
    assert!(KeyStore::set_with_storage(
        &config,
        "Gemini",
        "new-encrypted",
        CredentialStorage::Encrypted
    )
    .is_err());
    assert_eq!(
        KeyStore::get(&config, "OpenAI::plaintext")
            .expect("plaintext available")
            .as_deref(),
        Some("plaintext-second")
    );
    KeyStore::unlock_vault(&config, "test-only-vault-password").expect("unlock");
    assert_eq!(
        KeyStore::get(&config, "OpenAI")
            .expect("preserved encrypted")
            .as_deref(),
        Some("encrypted-original")
    );
    assert!(KeyStore::set_with_storage(
        &config,
        "OpenAI",
        "replacement",
        CredentialStorage::Plaintext
    )
    .is_err());
    assert_eq!(
        crate::ui::provider_display_name("OpenAI"),
        "OpenAI (encrypted)"
    );
    assert_eq!(
        crate::ui::provider_display_name("OpenAI::plaintext"),
        "OpenAI"
    );
}

#[test]
fn app_data_key_stays_encrypted_across_credential_conversion_and_restart() {
    let _guard = env_lock().lock().expect("env lock");
    let env = TestEnv::new("application-master-key");
    let config = env.initialized_config();
    assert!(KeyStore::protection_key(false).is_err());
    let key = KeyStore::protection_key(true).expect("create master");
    KeyStore::set_storage_preference(&config, CredentialStorage::Plaintext)
        .expect("convert providers only");
    assert!(KeyStore::list_secrets(&config)
        .expect("machine secrets")
        .is_empty());
    let connection = rusqlite::Connection::open(env.database_path()).expect("database");
    let encrypted: bool = connection
        .query_row(
            "SELECT encrypted FROM tcui_credentials WHERE kind='app_data_key' AND id='master'",
            [],
            |row| row.get(0),
        )
        .expect("master metadata");
    assert!(encrypted);
    drop(connection);
    KeyStore::lock_vault(&config).expect("lock");
    assert!(KeyStore::protection_key(false).is_err());
    KeyStore::unlock_vault(&config, "test-only-vault-password").expect("unlock");
    assert_eq!(*KeyStore::protection_key(false).expect("same master"), *key);
}

#[test]
fn oauth_refresh_preserves_each_connections_storage_instead_of_current_default() {
    let _guard = env_lock().lock().expect("env lock");
    let env = TestEnv::new("oauth-mixed-storage");
    let config = env.initialized_config();
    let mut encrypted = OAuthCredential {
        provider: "Codex".into(),
        access_token: "initial-encrypted".into(),
        refresh_token: Some("refresh-encrypted".into()),
        expires_at: chrono::Utc::now() + chrono::Duration::hours(1),
        account_id: Some("encrypted-account".into()),
        ownership: OAuthCredentialOwnership::Tcui,
        source: OAuthCredentialSource::NativeOAuth,
    };
    KeyStore::set_storage_default(&config, CredentialStorage::Plaintext)
        .expect("default changed during login");
    KeyStore::upsert_oauth_with_storage(&config, &encrypted, CredentialStorage::Encrypted)
        .expect("selected encryption survives changed default");
    let mut plaintext = encrypted.clone();
    plaintext.provider = "Codex::plaintext".into();
    plaintext.access_token = "initial-plaintext".into();
    KeyStore::upsert_oauth(&config, &plaintext).expect("plaintext OAuth");
    encrypted.access_token = "refreshed-encrypted".into();
    KeyStore::upsert_oauth(&config, &encrypted).expect("refresh remains encrypted");
    KeyStore::set_storage_default(&config, CredentialStorage::Encrypted)
        .expect("encrypted default");
    KeyStore::lock_vault(&config).expect("lock");
    plaintext.access_token = "refreshed-plaintext".into();
    KeyStore::upsert_oauth(&config, &plaintext).expect("plaintext refresh while locked");
    assert!(KeyStore::get_oauth(&config, "Codex").is_err());
    assert_eq!(
        KeyStore::get_oauth(&config, "Codex::plaintext").expect("plaintext retained"),
        Some(plaintext)
    );
    assert_eq!(
        KeyStore::provider_storage(&config, "Codex").expect("encrypted metadata"),
        Some(CredentialStorage::Encrypted)
    );
    KeyStore::unlock_vault(&config, "test-only-vault-password").expect("unlock");
    assert_eq!(
        KeyStore::get_oauth(&config, "Codex").expect("encrypted retained"),
        Some(encrypted)
    );
}

#[test]
fn adding_after_whole_store_conversion_keeps_every_existing_connection() {
    let _guard = env_lock().lock().expect("env lock");
    let env = TestEnv::new("connection-id-after-conversion");
    let config = env.initialized_config();
    KeyStore::set(&config, "OpenAI", "first-encrypted").expect("first");
    KeyStore::set_with_storage(
        &config,
        "OpenAI::plaintext",
        "second-plaintext",
        CredentialStorage::Plaintext,
    )
    .expect("second");
    KeyStore::set_storage_preference(&config, CredentialStorage::Encrypted)
        .expect("whole-store encryption");
    KeyStore::lock_vault(&config).expect("lock");
    let id = KeyStore::connection_id(&config, "OpenAI", CredentialStorage::Plaintext)
        .expect("new identity");
    assert_eq!(id, "OpenAI::plaintext:2");
    assert_eq!(provider_base_name(&id), "OpenAI");
    KeyStore::set_with_storage(
        &config,
        &id,
        "third-plaintext",
        CredentialStorage::Plaintext,
    )
    .expect("third");
    assert!(KeyStore::get(&config, "OpenAI::plaintext").is_err());
    KeyStore::unlock_vault(&config, "test-only-vault-password").expect("unlock");
    assert_eq!(
        KeyStore::get(&config, "OpenAI")
            .expect("first retained")
            .as_deref(),
        Some("first-encrypted")
    );
    assert_eq!(
        KeyStore::get(&config, "OpenAI::plaintext")
            .expect("second retained")
            .as_deref(),
        Some("second-plaintext")
    );
    assert_eq!(
        KeyStore::get(&config, &id)
            .expect("third retained")
            .as_deref(),
        Some("third-plaintext")
    );
}

#[test]
fn oauth_api_key_creation_keeps_explicit_storage_when_default_changes_mid_login() {
    let _guard = env_lock().lock().expect("env lock");
    let env = TestEnv::new("pkce-selected-storage");
    let config = env.initialized_config();
    let encrypted = Credential::ApiKey(
        ApiKeyCredential::new(
            "OpenRouter OAuth",
            "encrypted-pkce",
            ApiKeyCredentialOwnership::Tcui,
            ApiKeyCredentialSource::OpenRouterPkce,
        )
        .expect("credential"),
    );
    KeyStore::set_storage_default(&config, CredentialStorage::Plaintext)
        .expect("default changes during login");
    KeyStore::upsert_credential_with_storage(&config, &encrypted, CredentialStorage::Encrypted)
        .expect("selected encryption");
    KeyStore::lock_vault(&config).expect("lock");
    assert!(KeyStore::get_api_key_credential(&config, "OpenRouter OAuth").is_err());
    let second_encrypted = Credential::ApiKey(
        ApiKeyCredential::new(
            "Other OAuth",
            "new-encrypted-pkce",
            ApiKeyCredentialOwnership::Tcui,
            ApiKeyCredentialSource::OpenRouterPkce,
        )
        .expect("credential"),
    );
    assert!(KeyStore::upsert_credential_with_storage(
        &config,
        &second_encrypted,
        CredentialStorage::Encrypted
    )
    .is_err());
    assert!(!KeyStore::has_credential(&config, "Other OAuth").expect("not saved"));
    let plaintext = Credential::ApiKey(
        ApiKeyCredential::new(
            "OpenRouter OAuth::plaintext",
            "plaintext-pkce",
            ApiKeyCredentialOwnership::Tcui,
            ApiKeyCredentialSource::OpenRouterPkce,
        )
        .expect("credential"),
    );
    KeyStore::set_storage_default(&config, CredentialStorage::Encrypted)
        .expect("default changes again");
    KeyStore::upsert_credential_with_storage(&config, &plaintext, CredentialStorage::Plaintext)
        .expect("explicit plaintext while locked");
    assert!(
        KeyStore::get_api_key_credential(&config, "OpenRouter OAuth::plaintext")
            .expect("plaintext readable")
            .is_some()
    );
}
