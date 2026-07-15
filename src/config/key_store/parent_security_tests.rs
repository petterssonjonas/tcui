use super::persistence::write_file_with_after_final_check;
use super::test_support::{env_lock, TestEnv};
use super::*;
use std::collections::BTreeMap;
use std::path::PathBuf;

#[cfg(unix)]
fn config_with_symlink_parent(env: &TestEnv) -> (AppConfig, PathBuf) {
    use std::os::unix::fs::symlink;

    let mut config = env.config();
    let original_path = PathBuf::from(config.key_file.as_deref().expect("key file path"));
    let config_root = original_path
        .parent()
        .and_then(|parent| parent.parent())
        .expect("config root");
    let target_parent = config_root.join("target");
    let link_parent = config_root.join("link");
    std::fs::create_dir_all(&target_parent).expect("create target parent");
    symlink(&target_parent, &link_parent).expect("create parent symlink");
    let target_path = target_parent.join("keys.toml");
    config.key_file = Some(link_parent.join("keys.toml").display().to_string());
    (config, target_path)
}

#[cfg(unix)]
#[test]
fn get_rejects_symlink_parent_component_without_reading_target() {
    let _guard = env_lock().lock().expect("env lock poisoned");
    let env = TestEnv::new("keys-parent-symlink-read");
    let (config, target_path) = config_with_symlink_parent(&env);
    let encrypted = crate::storage::Storage::encrypt_shared_text("legacy-api-key")
        .expect("encrypt legacy API key");
    let fixture = format!("[keys]\nOpenAI = \"{encrypted}\"\n");
    std::fs::write(&target_path, &fixture).expect("write target key file");

    let error = KeyStore::get(&config, "OpenAI").expect_err("symlink parent must fail");

    assert!(matches!(
        error.downcast_ref::<KeyStoreError>(),
        Some(KeyStoreError::UnsafePath)
    ));
    assert_eq!(
        std::fs::read_to_string(target_path).expect("read target key file"),
        fixture
    );
}

#[cfg(unix)]
#[test]
fn save_keys_rejects_symlink_parent_component_without_writing_target() {
    let _guard = env_lock().lock().expect("env lock poisoned");
    let env = TestEnv::new("keys-parent-symlink-save");
    let (config, target_path) = config_with_symlink_parent(&env);
    let encrypted = crate::storage::Storage::encrypt_shared_text("legacy-api-key")
        .expect("encrypt legacy API key");
    let fixture = format!("[keys]\nOpenAI = \"{encrypted}\"\n");
    std::fs::write(&target_path, &fixture).expect("write target key file");

    let error = KeyStore::save_keys(
        &config,
        &[("OpenAI".to_string(), "replacement-api-key".to_string())],
    )
    .expect_err("symlink parent must fail");

    assert!(matches!(
        error.downcast_ref::<KeyStoreError>(),
        Some(KeyStoreError::UnsafePath)
    ));
    assert_eq!(
        std::fs::read_to_string(target_path).expect("read target key file"),
        fixture
    );
}

#[cfg(unix)]
#[test]
fn write_restores_previous_file_when_parent_moves_after_final_identity_check() {
    use std::os::unix::fs::symlink;

    let _guard = env_lock().lock().expect("env lock poisoned");
    let env = TestEnv::new("keys-parent-moves-after-final-check");
    let config = env.config();
    let path = PathBuf::from(config.key_file.as_deref().expect("key file path"));
    let parent = path.parent().expect("key file parent").to_path_buf();
    let archived_parent = parent.with_file_name("parent-after-final-check");
    let replacement_target = env.data_home().join("replacement-target");
    let original = "[keys]\nOpenAI = \"enc:v1:original\"\n";
    std::fs::create_dir_all(&parent).expect("create key file parent");
    std::fs::create_dir_all(&replacement_target).expect("create replacement target");
    std::fs::write(&path, original).expect("write original credential file");
    let replacement = StoredKeysFile {
        version: 1,
        keys: BTreeMap::new(),
        oauth: BTreeMap::new(),
        credentials: BTreeMap::new(),
    };

    let error = write_file_with_after_final_check(&path, &replacement, || {
        std::fs::rename(&parent, &archived_parent).expect("move original parent");
        symlink(&replacement_target, &parent).expect("replace parent with symlink");
    })
    .expect_err("moved parent must roll back committed replacement");

    assert!(matches!(error, KeyStoreError::ParentChanged));
    assert_eq!(
        std::fs::read_to_string(archived_parent.join("keys.toml"))
            .expect("read restored credential file"),
        original
    );
    assert!(!replacement_target.join("keys.toml").exists());
}
