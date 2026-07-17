use std::fs::OpenOptions;

use crate::config::vault_profiles::VaultProfile;
use crate::config::AppConfig;

use super::super::{ChatSavedLegacyReset, LegacyResetError};
use super::support::TestRoot;

#[test]
fn chat_saved_legacy_reset_refuses_saved_root_containing_a_protected_vault() {
    // Given
    let root = TestRoot::new("protected-overlap");
    let saved_root = root.create_dir("saved");
    let protected_vault = root.create_dir("saved/vault-profile");
    root.write("saved/legacy.md", b"must survive overlap");
    root.write("saved/vault-profile/sentinel.md", b"vault sentinel");
    root.write(
        "cache/tcui/chat-files/1/payload",
        b"cache must survive overlap",
    );
    let mut config = root.config(&saved_root);
    let profile = VaultProfile::new("Nested Vault", &protected_vault).expect("create profile");
    config.vaults.insert(profile.id.clone(), profile);
    let reset = root.reset(&config, None);

    // When
    let error = reset.run().expect_err("protected overlap must fail closed");

    // Then
    assert!(matches!(
        error,
        LegacyResetError::ProtectedRootOverlap { .. }
    ));
    assert_eq!(root.read("saved/legacy.md"), b"must survive overlap");
    assert_eq!(
        root.read("saved/vault-profile/sentinel.md"),
        b"vault sentinel"
    );
    assert_eq!(
        root.read("cache/tcui/chat-files/1/payload"),
        b"cache must survive overlap"
    );
    assert!(!root.marker_path().exists());
}

#[test]
fn chat_saved_legacy_reset_protects_legacy_vault_path() {
    // Given
    let root = TestRoot::new("legacy-vault-overlap");
    let legacy_vault = root.create_dir("legacy-vault");
    root.write("legacy-vault/sentinel.md", b"legacy vault sentinel");
    root.write("cache/tcui/chat-files/1/payload", b"cache sentinel");
    let mut config = root.config(&legacy_vault);
    config.vault_path = Some(legacy_vault.display().to_string());
    let reset = root.reset(&config, None);

    // When
    let error = reset
        .run()
        .expect_err("legacy Vault overlap must fail closed");

    // Then
    assert!(matches!(
        error,
        LegacyResetError::ProtectedRootOverlap { .. }
    ));
    assert_eq!(
        root.read("legacy-vault/sentinel.md"),
        b"legacy vault sentinel"
    );
    assert_eq!(
        root.read("cache/tcui/chat-files/1/payload"),
        b"cache sentinel"
    );
    assert!(!root.marker_path().exists());
}

#[test]
fn chat_saved_legacy_reset_protects_active_vault_root() {
    // Given
    let root = TestRoot::new("active-vault-overlap");
    let active_vault = root.create_dir("active-vault");
    root.write("active-vault/sentinel.md", b"active vault sentinel");
    root.write("cache/tcui/chat-files/1/payload", b"cache sentinel");
    let config = root.config(&active_vault);
    let reset = root.reset(&config, Some(&active_vault));

    // When
    let error = reset
        .run()
        .expect_err("active Vault overlap must fail closed");

    // Then
    assert!(matches!(
        error,
        LegacyResetError::ProtectedRootOverlap { .. }
    ));
    assert_eq!(
        root.read("active-vault/sentinel.md"),
        b"active vault sentinel"
    );
    assert_eq!(
        root.read("cache/tcui/chat-files/1/payload"),
        b"cache sentinel"
    );
    assert!(!root.marker_path().exists());
}

#[test]
fn chat_saved_legacy_reset_protects_fallback_vault_root() {
    // Given
    let root = TestRoot::new("fallback-vault-overlap");
    let fallback_vault = root.create_dir("data/tcui/vault");
    root.write("data/tcui/vault/sentinel.md", b"fallback vault sentinel");
    root.write("cache/tcui/chat-files/1/payload", b"cache sentinel");
    let config = root.config(&fallback_vault);
    let reset = root.reset(&config, None);

    // When
    let error = reset
        .run()
        .expect_err("fallback Vault overlap must fail closed");

    // Then
    assert!(matches!(
        error,
        LegacyResetError::ProtectedRootOverlap { .. }
    ));
    assert_eq!(
        root.read("data/tcui/vault/sentinel.md"),
        b"fallback vault sentinel"
    );
    assert_eq!(
        root.read("cache/tcui/chat-files/1/payload"),
        b"cache sentinel"
    );
    assert!(!root.marker_path().exists());
}

#[test]
fn chat_saved_legacy_reset_refuses_home_and_filesystem_roots() {
    // Given
    let root = TestRoot::new("unsafe-roots");
    root.create_dir("home");
    root.write(
        "cache/tcui/chat-files/1/payload",
        b"must survive unsafe root",
    );
    let home_config = root.config(&root.path("home"));
    let home_reset = root.reset(&home_config, None);
    let filesystem_config = root.config(std::path::Path::new("/"));
    let filesystem_reset = root.reset(&filesystem_config, None);

    // When
    let home_error = home_reset.run().expect_err("home root must be refused");
    let filesystem_error = filesystem_reset
        .run()
        .expect_err("filesystem root must be refused");

    // Then
    assert!(matches!(
        home_error,
        LegacyResetError::UnsafeSavedRoot { .. }
    ));
    assert!(matches!(
        filesystem_error,
        LegacyResetError::UnsafeSavedRoot { .. }
    ));
    assert_eq!(
        root.read("cache/tcui/chat-files/1/payload"),
        b"must survive unsafe root"
    );
    assert!(!root.marker_path().exists());
}

#[test]
fn chat_saved_legacy_reset_refuses_relative_saved_root_without_current_dir_fallback() {
    // Given
    let root = TestRoot::new("relative-saved-root");
    let config = AppConfig {
        saved_files_dir: Some("relative/saved".to_string()),
        artifact_save_dir: None,
        ..AppConfig::default()
    };

    // When
    let error = ChatSavedLegacyReset::prepare(&config, None, root.environment())
        .expect_err("relative Saved root must be refused");

    // Then
    assert!(matches!(
        error,
        LegacyResetError::SavedRootNotAbsolute { .. }
    ));
    assert!(!root.marker_path().exists());
}

#[test]
fn chat_saved_legacy_reset_resolves_missing_saved_config_to_explicit_data_root() {
    // Given
    let root = TestRoot::new("default-saved-root");
    let config = AppConfig {
        saved_files_dir: None,
        artifact_save_dir: None,
        ..AppConfig::default()
    };

    // When
    let reset = ChatSavedLegacyReset::prepare(&config, None, root.environment())
        .expect("prepare default Saved root");

    // Then
    assert_eq!(reset.saved_root(), root.path("data/tcui/artifacts"));
}

#[test]
fn chat_saved_legacy_reset_returns_busy_without_mutation_when_lock_is_contended() {
    // Given
    let root = TestRoot::new("lock-contention");
    let saved_root = root.create_dir("saved");
    root.write("saved/legacy.md", b"saved must survive contention");
    root.write(
        "cache/tcui/chat-files/1/payload",
        b"cache must survive contention",
    );
    let config = root.config(&saved_root);
    let reset = root.reset(&config, None);
    std::fs::create_dir_all(root.lock_path().parent().expect("lock parent"))
        .expect("create migration directory");
    let lock = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(root.lock_path())
        .expect("open migration lock");
    lock.try_lock().expect("hold migration lock");

    // When
    let error = reset.run().expect_err("contended reset must fail");

    // Then
    assert!(matches!(error, LegacyResetError::MigrationBusy));
    assert_eq!(
        root.read("saved/legacy.md"),
        b"saved must survive contention"
    );
    assert_eq!(
        root.read("cache/tcui/chat-files/1/payload"),
        b"cache must survive contention"
    );
    assert!(!root.marker_path().exists());
}
