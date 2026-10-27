#![cfg(test)]

use super::*;

#[test]
fn vault_config_migration_discards_external_selection_without_importing_or_mutating_files() {
    let _guard = env_lock().lock().expect("env lock poisoned");
    let home = ConfigHome::new("vault-config-external");
    let external = home.root.join("external");
    std::fs::create_dir_all(&external).expect("create external directory");
    std::fs::write(external.join("sentinel.md"), b"external content").expect("seed external file");
    let original = format!(
        "theme = \"legacy-theme\"\ndefault_model = \"retained-model\"\nvault_path = {path:?}\nartifact_save_dir = {path:?}\ndefault_vault_id = \"obsolete-id\"\nvault_operation_retention_days = 14\n[vaults.obsolete-id]\nroot = {path:?}\ndisplay_name = \"External\"\n",
        path = external.display().to_string(),
    );
    home.seed_toml(&original);

    let config = AppConfig::load().expect("migrate external config");
    crate::storage::saved_file_migration::migrate_saved_files(&config)
        .expect("migrate native Saved files only");
    let rewritten = std::fs::read(home.config_path()).expect("read migrated config");
    let second = AppConfig::load().expect("reload migrated config");

    assert_eq!(config.theme, "legacy-theme");
    assert_eq!(second.default_chat.model, "retained-model");
    assert!(config.saved_files_dir.is_none());
    assert_eq!(
        std::fs::read(home.backup_path()).expect("read original backup"),
        original.as_bytes()
    );
    assert_eq!(
        std::fs::read(home.config_path()).expect("read stable config"),
        rewritten
    );
    assert_eq!(
        std::fs::read(external.join("sentinel.md")).expect("read external sentinel"),
        b"external content"
    );
    assert!(!crate::protection::internal_vault_root()
        .join("sentinel.md")
        .exists());
    let persisted: toml::Value =
        toml::from_str(std::str::from_utf8(&rewritten).expect("UTF-8 config"))
            .expect("parse config");
    for key in [
        "vaults",
        "default_vault_id",
        "vault_path",
        "artifact_save_dir",
        "vault_operation_retention_days",
    ] {
        assert!(
            persisted.get(key).is_none(),
            "obsolete field retained: {key}"
        );
    }
}

#[test]
fn vault_config_migration_json_keeps_original_and_explicit_saved_directory() {
    let _guard = env_lock().lock().expect("env lock poisoned");
    let home = ConfigHome::new("vault-config-json-saved");
    let saved = home.root.join("legacy-saved");
    std::fs::create_dir_all(&saved).expect("create Saved directory");
    std::fs::write(saved.join("saved.md"), b"authorized Saved content").expect("seed Saved file");
    let original = serde_json::json!({
        "theme": "retained-theme",
        "saved_files_dir": saved,
        "vault_path": "/unavailable/external",
        "artifact_save_dir": "/other/external",
        "vaults": { "invalid-old-id": { "root": "/unavailable/external" } },
        "default_vault_id": "invalid-old-id"
    })
    .to_string();
    home.seed_json(&original);

    let config = AppConfig::load().expect("migrate JSON config");
    let reloaded = AppConfig::load().expect("reload explicit Saved config");
    crate::storage::saved_file_migration::migrate_saved_files(&reloaded)
        .expect("copy explicitly configured Saved directory");

    assert_eq!(config.saved_files_dir, Some(saved.display().to_string()));
    assert_eq!(reloaded.saved_files_dir, config.saved_files_dir);
    assert_eq!(
        std::fs::read_to_string(home.json_path()).expect("read original JSON"),
        original
    );
    assert_eq!(
        std::fs::read(saved.join("saved.md")).expect("read retained source"),
        b"authorized Saved content"
    );
    assert_eq!(
        std::fs::read(crate::protection::internal_vault_root().join("saved.md"))
            .expect("read native copy"),
        b"authorized Saved content"
    );
}

#[test]
fn vault_config_migration_preserves_visibility_and_native_content() {
    let _guard = env_lock().lock().expect("env lock poisoned");
    let home = ConfigHome::new("vault-config-native");
    let native = crate::protection::internal_vault_root();
    std::fs::create_dir_all(&native).expect("create native Vault");
    std::fs::write(native.join("existing.md"), b"native content").expect("seed native content");
    home.seed_toml("show_saved_files = false\nshow_memories = false\nshow_markdown_vault = false\nvaults = \"obsolete malformed catalog\"\n");

    let config = AppConfig::load().expect("remove obsolete catalog");
    let reloaded = AppConfig::load().expect("reload migrated visibility");

    assert!(!config.show_memories);
    assert!(!config.show_vault);
    assert!(!reloaded.show_vault);
    assert_eq!(
        std::fs::read(native.join("existing.md")).expect("read native content"),
        b"native content"
    );
}

#[test]
fn chat_defaults_migrate_together_and_new_selection_wins_over_legacy_keys() {
    let _guard = env_lock().lock().expect("env lock poisoned");
    let home = ConfigHome::new("default-chat-migration");
    home.seed_toml("default_provider = \"Saved connection\"\ndefault_model = \"saved-model\"\n");
    let migrated = AppConfig::load().expect("migrate selection");
    assert_eq!(
        migrated.default_chat,
        crate::config::ChatSelection {
            provider: "Saved connection".into(),
            model: "saved-model".into(),
            effort: None,
        }
    );
    let rewritten = std::fs::read_to_string(home.config_path()).expect("saved config");
    assert!(!rewritten.contains("default_provider"));
    assert!(!rewritten.contains("default_model"));
    assert_eq!(
        AppConfig::load().expect("reload").default_chat,
        migrated.default_chat
    );
    home.seed_toml("default_provider = \"obsolete\"\ndefault_model = \"obsolete\"\n[default_chat]\nprovider = \"Saved connection\"\nmodel = \"explicit-model\"\neffort = \"medium\"\n");
    let selected = AppConfig::load().expect("prefer new combination");
    assert_eq!(selected.default_chat.provider, "Saved connection");
    assert_eq!(selected.default_chat.model, "explicit-model");
    assert_eq!(selected.default_chat.effort.as_deref(), Some("medium"));
    assert_eq!(
        AppConfig::load().expect("roundtrip").default_chat,
        selected.default_chat
    );
}
