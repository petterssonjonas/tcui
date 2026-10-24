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
    assert_eq!(first.default_vault_id, second.default_vault_id);
    assert_eq!(
        std::fs::read_to_string(home.json_path()).expect("read retained JSON"),
        original.replace("from-json", "modified-json")
    );
}

#[test]
fn vault_config_migration_normal_save_removes_legacy_keys() {
    let _guard = env_lock().lock().expect("env lock poisoned");
    let home = ConfigHome::new("vault-config-legacy-save");
    let config = AppConfig {
        vault_path: Some("/legacy/vault".to_string()),
        artifact_save_dir: Some("/legacy/artifacts".to_string()),
        ..AppConfig::default()
    };

    config.save().expect("save config");

    let persisted = std::fs::read_to_string(home.config_path()).expect("read saved config");
    assert!(!persisted.contains("vault_path"));
    assert!(!persisted.contains("artifact_save_dir"));
}

#[cfg(unix)]
#[test]
fn config_save_and_rollback_ignore_planted_fixed_temporary_links() {
    use std::os::unix::fs::{symlink, PermissionsExt};

    let _guard = env_lock().lock().unwrap();
    let home = ConfigHome::new("config-temporary-links");
    let mut config = AppConfig::default();
    config.save().unwrap();
    let sentinel = home.root.join("sentinel");
    std::fs::write(&sentinel, b"dummy private sentinel").unwrap();
    for name in [".config.toml.tmp", ".config.toml.restore.tmp"] {
        symlink(&sentinel, home.tcui_dir().join(name)).unwrap();
    }

    config.theme = "first-change".to_string();
    config.save().unwrap();
    let committed = std::fs::read(home.config_path()).unwrap();
    config.theme = "not-committed".to_string();
    inject_failure(TestFailurePoint::ParentSync);
    let error = config.save().unwrap_err();
    assert_injected_failure(&error, "parent sync");
    assert_eq!(std::fs::read(home.config_path()).unwrap(), committed);
    assert_eq!(std::fs::read(&sentinel).unwrap(), b"dummy private sentinel");
    for name in [".config.toml.tmp", ".config.toml.restore.tmp"] {
        assert!(std::fs::symlink_metadata(home.tcui_dir().join(name))
            .unwrap()
            .is_symlink());
    }
    let metadata = std::fs::symlink_metadata(home.config_path()).unwrap();
    assert!(metadata.is_file() && !metadata.is_symlink());
    assert_eq!(metadata.permissions().mode() & 0o777, 0o600);
}

#[cfg(unix)]
#[test]
fn config_load_and_save_reject_linked_destinations_locks_and_parents() {
    use std::os::unix::fs::symlink;

    let _guard = env_lock().lock().unwrap();
    for leaf in ["config.toml", ".config.lock"] {
        let home = ConfigHome::new("config-leaf-link");
        let config = AppConfig::default();
        let original = serialized_config(&config);
        let sentinel = home.root.join("sentinel");
        std::fs::write(&sentinel, &original).unwrap();
        let link = home.tcui_dir().join(leaf);
        symlink(&sentinel, &link).unwrap();
        assert!(AppConfig::load().is_err());
        assert!(config.save().is_err());
        assert_eq!(std::fs::read_to_string(&sentinel).unwrap(), original);
        assert!(std::fs::symlink_metadata(link).unwrap().is_symlink());
    }

    let home = ConfigHome::new("config-parent-link");
    let config = AppConfig::default();
    let original = serialized_config(&config);
    let outside = home.root.join("outside");
    std::fs::create_dir(&outside).unwrap();
    let sentinel = outside.join("config.toml");
    std::fs::write(&sentinel, &original).unwrap();
    std::fs::remove_dir(home.tcui_dir()).unwrap();
    symlink(&outside, home.tcui_dir()).unwrap();
    assert!(AppConfig::load().is_err());
    assert!(config.save().is_err());
    assert_eq!(std::fs::read_to_string(sentinel).unwrap(), original);
    assert!(!outside.join(".config.lock").exists());
}

#[cfg(unix)]
#[test]
fn migration_rejects_a_linked_backup_without_changing_either_original() {
    use std::os::unix::fs::symlink;

    let _guard = env_lock().lock().unwrap();
    let home = ConfigHome::new("config-backup-link");
    let original = "theme = \"legacy-theme\"\nvault_path = \"/missing/legacy/vault\"\n";
    home.seed_toml(original);
    let sentinel = home.root.join("sentinel");
    std::fs::write(&sentinel, b"dummy backup sentinel").unwrap();
    symlink(&sentinel, home.backup_path()).unwrap();
    assert!(AppConfig::load().is_err());
    assert_eq!(
        std::fs::read_to_string(home.config_path()).unwrap(),
        original
    );
    assert_eq!(std::fs::read(sentinel).unwrap(), b"dummy backup sentinel");
}

#[cfg(unix)]
#[test]
fn migration_backup_lock_and_reload_rollback_are_private_regular_files() {
    use std::os::unix::fs::PermissionsExt;

    let _guard = env_lock().lock().unwrap();
    let home = ConfigHome::new("config-private-recovery");
    let original = "theme = \"legacy-theme\"\nvault_path = \"/missing/legacy/vault\"\n";
    home.seed_toml(original);
    inject_failure(TestFailurePoint::Reload);
    let error = AppConfig::load().unwrap_err();
    assert_injected_failure(&error, "reload");
    assert_eq!(
        std::fs::read_to_string(home.config_path()).unwrap(),
        original
    );
    assert_eq!(
        std::fs::read_to_string(home.backup_path()).unwrap(),
        original
    );
    for path in [
        home.config_path(),
        home.backup_path(),
        home.tcui_dir().join(".config.lock"),
    ] {
        let metadata = std::fs::symlink_metadata(path).unwrap();
        assert!(metadata.is_file() && !metadata.is_symlink());
        assert_eq!(metadata.permissions().mode() & 0o777, 0o600);
    }
}

#[test]
fn failed_backup_publication_retains_the_original_and_allows_a_complete_retry() {
    let _guard = env_lock().lock().unwrap();
    let home = ConfigHome::new("config-backup-publication-failure");
    let original = "theme = \"legacy-theme\"\nvault_path = \"/missing/legacy/vault\"\n";
    home.seed_toml(original);
    inject_failure(TestFailurePoint::ParentSync);

    let error = AppConfig::load().unwrap_err();
    assert_injected_failure(&error, "parent sync");
    assert_eq!(
        std::fs::read_to_string(home.config_path()).unwrap(),
        original
    );
    assert!(!home.backup_path().exists());
    let mut names: Vec<_> = std::fs::read_dir(home.tcui_dir())
        .unwrap()
        .map(|entry| entry.unwrap().file_name())
        .collect();
    names.sort();
    assert_eq!(
        names,
        [
            std::ffi::OsString::from(".config.lock"),
            std::ffi::OsString::from("config.toml")
        ]
    );

    assert_eq!(AppConfig::load().unwrap().theme, "legacy-theme");
    assert_eq!(
        std::fs::read_to_string(home.backup_path()).unwrap(),
        original
    );
}
