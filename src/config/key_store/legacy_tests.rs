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
