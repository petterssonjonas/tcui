#![cfg(test)]

use super::*;

#[test]
fn vault_config_migration_toml_and_json_legacy_inputs_produce_identical_stable_config() {
    let _guard = env_lock().lock().expect("env lock poisoned");
    let vault_root = unique_temp_dir("vault-config-shared-root");
    std::fs::create_dir_all(&vault_root).expect("create vault root");
    let legacy_toml = format!(
        "vault_path = {:?}\nartifact_save_dir = {:?}\n",
        vault_root.display().to_string(),
        "/tmp/legacy-saved"
    );
    let legacy_json = serde_json::json!({
        "vault_path": vault_root,
        "artifact_save_dir": "/tmp/legacy-saved"
    })
    .to_string();

    let toml_config = {
        let home = ConfigHome::new("vault-config-toml-legacy");
        home.seed_toml(&legacy_toml);
        AppConfig::load().expect("migrate TOML")
    };
    let json_config = {
        let home = ConfigHome::new("vault-config-json-legacy");
        home.seed_json(&legacy_json);
        AppConfig::load().expect("migrate JSON")
    };

    assert_eq!(toml_config.vaults, json_config.vaults);
    assert_eq!(toml_config.default_vault_id, json_config.default_vault_id);
    assert_eq!(toml_config.saved_files_dir, json_config.saved_files_dir);
    std::fs::remove_dir_all(vault_root).expect("cleanup vault root");
}

#[test]
fn vault_config_migration_first_profile_fallback_rewrites_only_once() {
    let _guard = env_lock().lock().expect("env lock poisoned");
    let home = ConfigHome::new("vault-config-default-fallback");
    let vault_root = home.root.join("vault");
    std::fs::create_dir_all(&vault_root).expect("create vault root");
    let config = profile_config("First", &vault_root);
    let expected_id = config.default_vault_id.clone();
    let mut value: toml::Value = toml::from_str(&serialized_config(&config)).expect("parse TOML");
    value
        .as_table_mut()
        .expect("top-level table")
        .remove("default_vault_id");
    let original = toml::to_string_pretty(&value).expect("serialize fallback fixture");
    home.seed_toml(&original);

    let first = AppConfig::load().expect("load fallback config");
    let after_first = std::fs::read(home.config_path()).expect("read first rewrite");
    let backup_after_first = std::fs::read(home.backup_path()).expect("read migration backup");
    let second = AppConfig::load().expect("reload fallback config");

    assert_eq!(first.default_vault_id, expected_id);
    assert_eq!(second.default_vault_id, expected_id);
    assert_eq!(
        std::fs::read(home.config_path()).expect("read reload"),
        after_first
    );
    assert_eq!(
        std::fs::read(home.backup_path()).expect("read retained backup"),
        backup_after_first
    );
    assert_eq!(backup_after_first, original.as_bytes());
}

#[test]
fn vault_config_migration_visibility_flags_default_true() {
    let _guard = env_lock().lock().expect("env lock poisoned");
    let home = ConfigHome::new("vault-config-flag-defaults");
    home.seed_toml("theme = \"system\"\n");

    let config = AppConfig::load().expect("load defaults");

    assert!(config.show_saved_files);
    assert!(config.show_memories);
    assert!(config.show_markdown_vault);
}

#[test]
fn vault_config_migration_unavailable_persisted_root_survives_unchanged() {
    let _guard = env_lock().lock().expect("env lock poisoned");
    let home = ConfigHome::new("vault-config-unavailable-root");
    let vault_root = home.root.join("vanishing-vault");
    std::fs::create_dir_all(&vault_root).expect("create vault root");
    let config = profile_config("Missing", &vault_root);
    let id = config.default_vault_id.clone().expect("default ID");
    let canonical_root = config.vaults.get(&id).expect("profile").root.clone();
    home.seed_toml(&serialized_config(&config));
    std::fs::remove_dir_all(&vault_root).expect("remove vault root");

    let loaded = AppConfig::load().expect("load unavailable profile");
    let profile = loaded.vaults.get(&id).expect("loaded profile");

    assert_eq!(profile.root, canonical_root);
    assert!(!profile.available);
}

#[test]
fn vault_config_migration_duplicate_profiles_fail_closed_without_rewrite() {
    let _guard = env_lock().lock().expect("env lock poisoned");
    let home = ConfigHome::new("vault-config-duplicate");
    let vault_root = home.root.join("vault");
    std::fs::create_dir_all(&vault_root).expect("create vault root");
    let first = VaultProfile::new("Duplicate", &vault_root).expect("create first profile");
    let second = VaultProfile::from_persisted(VaultProfileId::new(), " duplicate ", &vault_root)
        .expect("create duplicate persisted profile");
    let original = serialized_config(&AppConfig {
        default_vault_id: Some(first.id.clone()),
        vaults: HashMap::from([(first.id.clone(), first), (second.id.clone(), second)]),
        ..AppConfig::default()
    });
    home.seed_toml(&original);

    let result = AppConfig::load();

    assert!(result.is_err());
    assert_eq!(
        std::fs::read_to_string(home.config_path()).expect("read original"),
        original
    );
    assert!(!home.backup_path().exists());
}

#[test]
fn vault_config_migration_malformed_profile_id_fails_closed() {
    let _guard = env_lock().lock().expect("env lock poisoned");
    let home = ConfigHome::new("vault-config-malformed-id");
    let original = "default_vault_id = \"not-an-id\"\n[vaults]\n";
    home.seed_toml(original);

    let result = AppConfig::load();

    assert!(result.is_err());
    assert_eq!(
        std::fs::read_to_string(home.config_path()).expect("read original"),
        original
    );
}

#[test]
fn vault_config_migration_nonempty_new_catalog_and_saved_dir_win_over_legacy() {
    let _guard = env_lock().lock().expect("env lock poisoned");
    let home = ConfigHome::new("vault-config-new-precedence");
    let vault_root = home.root.join("new-vault");
    std::fs::create_dir_all(&vault_root).expect("create vault root");
    let mut config = profile_config("Current", &vault_root);
    config.saved_files_dir = Some("/new/saved".to_string());
    let expected_id = config.default_vault_id.clone();
    let mut value: toml::Value = toml::from_str(&serialized_config(&config)).expect("parse config");
    let table = value.as_table_mut().expect("top-level table");
    table.insert(
        "vault_path".to_string(),
        toml::Value::String("/legacy/vault".to_string()),
    );
    table.insert(
        "artifact_save_dir".to_string(),
        toml::Value::String("/legacy/saved".to_string()),
    );
    home.seed_toml(&toml::to_string_pretty(&value).expect("serialize fixture"));

    let loaded = AppConfig::load().expect("load precedence config");

    assert_eq!(loaded.default_vault_id, expected_id);
    assert_eq!(loaded.saved_files_dir.as_deref(), Some("/new/saved"));
    assert_eq!(loaded.vaults.len(), 1);
}
