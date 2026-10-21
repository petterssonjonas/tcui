use super::test_support::{env_lock, TestEnv};
use super::*;

fn legacy_file(config: &AppConfig, text: &str) -> PathBuf {
    let path = PathBuf::from(config.key_file.as_deref().expect("path"));
    std::fs::create_dir_all(path.parent().expect("parent")).expect("mkdir");
    std::fs::write(&path, text).expect("legacy fixture");
    path
}

#[test]
fn future_legacy_version_is_rejected_without_modifying_source() {
    let _guard = env_lock().lock().expect("env lock");
    let env = TestEnv::new("vault-future-legacy");
    let config = env.initialized_config();
    let fixture = "version = 99\n[keys]\nOpenAI = 'legacy-canary'\n[oauth]\nFuture = 'opaque'\n";
    let path = legacy_file(&config, fixture);
    assert!(matches!(
        KeyStore::migrate_legacy(&config),
        Err(KeyStoreError::UnsupportedVersion { version: 99 })
    ));
    assert_eq!(std::fs::read_to_string(path).expect("source"), fixture);
    assert!(!KeyStore::has_credential(&config, "OpenAI").expect("presence"));
}

#[test]
fn legacy_import_is_explicit_atomic_idempotent_and_preserves_source() {
    let _guard = env_lock().lock().expect("env lock");
    let env = TestEnv::new("vault-legacy-import");
    let config = env.config();
    let fixture = "[keys]\nOpenAI = 'legacy-canary'\nMistral = 'other-canary'\n";
    let path = legacy_file(&config, fixture);
    assert_eq!(
        KeyStore::get(&config, "OpenAI").expect("no automatic import"),
        None
    );
    assert!(KeyStore::migrate_legacy(&config).is_err());
    KeyStore::initialize_vault(&config, "test-password").expect("initialize");
    assert_eq!(KeyStore::migrate_legacy(&config).expect("import"), 2);
    assert_eq!(KeyStore::migrate_legacy(&config).expect("repeat import"), 0);
    assert_eq!(
        KeyStore::get(&config, "OpenAI").expect("read").as_deref(),
        Some("legacy-canary")
    );
    assert_eq!(std::fs::read_to_string(path).expect("source"), fixture);
    assert!(!env.data_home().join("tcui/chat.key").exists());
}

#[test]
fn conflicting_legacy_import_never_overwrites_existing_credentials() {
    let _guard = env_lock().lock().expect("env lock");
    let env = TestEnv::new("vault-legacy-conflict");
    let config = env.initialized_config();
    legacy_file(
        &config,
        "[keys]\nAlpha = 'would-be-imported'\nOpenAI = 'legacy-canary'\n",
    );
    KeyStore::set(&config, "OpenAI", "newer-canary").expect("new credential");
    assert!(matches!(
        KeyStore::migrate_legacy(&config),
        Err(KeyStoreError::MigrationConflict)
    ));
    assert_eq!(KeyStore::get(&config, "Alpha").expect("rolled back"), None);
    assert_eq!(
        KeyStore::get(&config, "OpenAI")
            .expect("preserved")
            .as_deref(),
        Some("newer-canary")
    );
}

#[test]
fn missing_legacy_machine_key_is_not_recreated() {
    let _guard = env_lock().lock().expect("env lock");
    let env = TestEnv::new("vault-missing-legacy-key");
    let config = env.initialized_config();
    legacy_file(&config, "[keys]\nOpenAI = 'enc:v1:invalid'\n");
    assert!(matches!(
        KeyStore::migrate_legacy(&config),
        Err(KeyStoreError::KeyAccess)
    ));
    assert!(!env.data_home().join("tcui/chat.key").exists());
}

#[test]
fn machine_encrypted_api_and_oauth_legacy_records_migrate_together() {
    use crate::storage::crypto::{encrypt_serialized, encrypt_shared_text_with_key, SharedKey};
    use crate::storage::paths::TcuiDataPaths;
    let _guard = env_lock().lock().expect("env lock");
    let env = TestEnv::new("vault-encrypted-legacy");
    let config = env.initialized_config();
    let machine = SharedKey::load_or_create_default(&TcuiDataPaths::discover())
        .expect("legacy machine key")
        .key;
    let oauth = OAuthCredential {
        provider: "Codex".into(),
        access_token: "legacy-access".into(),
        refresh_token: Some("legacy-refresh".into()),
        expires_at: chrono::Utc::now(),
        account_id: None,
        ownership: OAuthCredentialOwnership::Tcui,
        source: OAuthCredentialSource::NativeOAuth,
    };
    let typed = ApiKeyCredential::new(
        "OpenRouter",
        "legacy-owned",
        ApiKeyCredentialOwnership::Tcui,
        ApiKeyCredentialSource::OpenRouterPkce,
    )
    .expect("typed");
    let mut file = format::StoredKeysFile::default();
    file.keys.insert(
        "OpenAI".into(),
        encrypt_shared_text_with_key(&machine, "legacy-api").expect("encrypt API"),
    );
    file.keys.insert(
        "OpenRouter".into(),
        encrypt_shared_text_with_key(&machine, "manual-openrouter").expect("encrypt manual key"),
    );
    file.oauth.insert(
        "Codex".into(),
        encrypt_serialized(
            &machine,
            "oauth:Codex",
            &StoredOAuthCredential::from(&oauth),
        )
        .expect("encrypt OAuth"),
    );
    file.credentials.insert(
        "OpenRouter".into(),
        encrypt_serialized(
            &machine,
            "credential:api_key:OpenRouter",
            &StoredApiKeyCredential::from(&typed),
        )
        .expect("encrypt typed"),
    );
    legacy_file(&config, &toml::to_string(&file).expect("serialize"));
    assert_eq!(KeyStore::migrate_legacy(&config).expect("import"), 4);
    assert_eq!(
        KeyStore::get(&config, "OpenAI").expect("API").as_deref(),
        Some("legacy-api")
    );
    assert_eq!(
        KeyStore::get_oauth(&config, "Codex").expect("OAuth"),
        Some(oauth)
    );
    let migrated = ApiKeyCredential::new(
        "OpenRouter OAuth",
        "legacy-owned",
        ApiKeyCredentialOwnership::Tcui,
        ApiKeyCredentialSource::OpenRouterPkce,
    )
    .expect("migrated typed");
    assert_eq!(
        KeyStore::get_api_key_credential(&config, "OpenRouter OAuth").expect("typed"),
        Some(migrated)
    );
    assert_eq!(
        KeyStore::get_api_key_credential(&config, "OpenRouter").expect("no obsolete identity"),
        None
    );
    assert_eq!(
        KeyStore::get(&config, "OpenRouter")
            .expect("manual API key")
            .as_deref(),
        Some("manual-openrouter")
    );
    assert_eq!(
        KeyStore::migrate_legacy(&config).expect("idempotent import"),
        0
    );
}

#[test]
fn future_database_version_rejects_reads_writes_and_reinitialization() {
    let _guard = env_lock().lock().expect("env lock");
    let env = TestEnv::new("vault-future-database");
    let config = env.initialized_config();
    KeyStore::set(&config, "OpenAI", "existing-canary").expect("persist");
    let conn = rusqlite::Connection::open(env.database_path()).expect("database");
    let original: Vec<u8> = conn
        .query_row(
            "SELECT payload FROM tcui_credentials WHERE id='OpenAI'",
            [],
            |row| row.get(0),
        )
        .expect("original ciphertext");
    conn.execute_batch(
        "PRAGMA ignore_check_constraints=ON; UPDATE tcui_credential_vault SET version=99",
    )
    .expect("future fixture");
    assert!(matches!(
        KeyStore::vault_status(&config),
        Err(KeyStoreError::UnsupportedVersion { version: 99 })
    ));
    assert!(KeyStore::get(&config, "OpenAI").is_err());
    assert!(matches!(
        KeyStore::set(&config, "OpenAI", "replacement"),
        Err(KeyStoreError::UnsupportedVersion { version: 99 })
    ));
    assert!(KeyStore::initialize_vault(&config, "replacement-password").is_err());
    assert!(KeyStore::set_storage_preference(&config, CredentialStorage::Plaintext).is_err());
    assert_eq!(
        conn.query_row("SELECT version FROM tcui_credential_vault", [], |row| row
            .get::<_, i64>(
            0
        ))
        .expect("version"),
        99
    );
    assert_eq!(
        conn.query_row(
            "SELECT payload FROM tcui_credentials WHERE id='OpenAI'",
            [],
            |row| row.get::<_, Vec<u8>>(0)
        )
        .expect("preserved ciphertext"),
        original
    );
}
