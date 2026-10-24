#![cfg(test)]

use std::fs::{File, OpenOptions};

use super::*;

#[test]
fn vault_config_migration_lock_contention_returns_config_busy() {
    let _guard = env_lock().lock().expect("env lock poisoned");
    let home = ConfigHome::new("vault-config-lock");
    let lock_path = home.tcui_dir().join(".config.lock");
    let lock_file = OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(&lock_path)
        .expect("open test lock");
    File::try_lock(&lock_file).expect("acquire test lock");

    let error = AppConfig::default()
        .save()
        .expect_err("reject contended save");

    assert!(matches!(
        error.downcast_ref::<ConfigError>(),
        Some(ConfigError::ConfigBusy { path }) if path == &lock_path
    ));
}

#[test]
fn vault_config_migration_sync_failure_preserves_readable_original() {
    let _guard = env_lock().lock().expect("env lock poisoned");
    let home = ConfigHome::new("vault-config-sync-failure");
    let original = serialized_config(&AppConfig::default());
    home.seed_toml(&original);
    let config = AppConfig {
        theme: "changed".to_string(),
        ..AppConfig::default()
    };
    inject_failure(TestFailurePoint::Sync);

    let error = config.save().expect_err("inject sync failure");

    assert_injected_failure(&error, "sync");
    assert_eq!(
        std::fs::read_to_string(home.config_path()).expect("read original"),
        original
    );
}

#[test]
fn vault_config_migration_rename_failure_preserves_readable_original() {
    let _guard = env_lock().lock().expect("env lock poisoned");
    let home = ConfigHome::new("vault-config-rename-failure");
    let original = serialized_config(&AppConfig::default());
    home.seed_toml(&original);
    let config = AppConfig {
        theme: "changed".to_string(),
        ..AppConfig::default()
    };
    inject_failure(TestFailurePoint::Rename);

    let error = config.save().expect_err("inject rename failure");

    assert_injected_failure(&error, "rename");
    assert_eq!(
        std::fs::read_to_string(home.config_path()).expect("read original"),
        original
    );
}

#[test]
fn vault_config_migration_parent_sync_failure_rolls_back_original() {
    let _guard = env_lock().lock().expect("env lock poisoned");
    let home = ConfigHome::new("vault-config-parent-sync-failure");
    let original = serialized_config(&AppConfig::default());
    home.seed_toml(&original);
    let config = AppConfig {
        theme: "changed".to_string(),
        ..AppConfig::default()
    };
    inject_failure(TestFailurePoint::ParentSync);

    let error = config.save().expect_err("inject parent sync failure");

    assert_injected_failure(&error, "parent sync");
    assert_eq!(
        std::fs::read_to_string(home.config_path()).expect("read original"),
        original
    );
}

#[test]
fn vault_config_migration_reload_failure_restores_exact_toml_original() {
    let _guard = env_lock().lock().expect("env lock poisoned");
    let home = ConfigHome::new("vault-config-reload-failure");
    let original = "theme = \"legacy-theme\"\nvault_path = \"/missing/legacy/vault\"\n";
    home.seed_toml(original);
    inject_failure(TestFailurePoint::Reload);

    let error = AppConfig::load().expect_err("inject reload failure");

    assert_injected_failure(&error, "reload");
    assert_eq!(
        std::fs::read_to_string(home.config_path()).expect("read original"),
        original
    );
    assert_eq!(
        std::fs::read_to_string(home.backup_path()).expect("read backup"),
        original
    );
}

#[test]
fn vault_config_migration_legacy_json_is_never_modified_and_toml_wins_later() {
    let _guard = env_lock().lock().expect("env lock poisoned");
    let home = ConfigHome::new("vault-config-json-retained");
    let original = r#"{
  "theme": "from-json",
  "vault_path": "/missing/json/vault"
}
"#;
    home.seed_json(original);

    let first = AppConfig::load().expect("migrate JSON");
    std::fs::write(
        home.json_path(),
        original.replace("from-json", "modified-json"),
    )
    .expect("modify retained JSON");
    let second = AppConfig::load().expect("prefer TOML");

    assert_eq!(first.theme, "from-json");
    assert_eq!(second.theme, "from-json");
    assert_eq!(
        std::fs::read_to_string(home.json_path()).expect("read retained JSON"),
        original.replace("from-json", "modified-json")
    );
}

#[test]
fn vault_config_migration_normal_save_removes_legacy_keys() {
    let _guard = env_lock().lock().expect("env lock poisoned");
    let home = ConfigHome::new("vault-config-legacy-save");
    let config = AppConfig::default();

    config.save().expect("save config");

    let persisted = std::fs::read_to_string(home.config_path()).expect("read saved config");
    assert!(!persisted.contains("vault_path"));
    assert!(!persisted.contains("artifact_save_dir"));
}
