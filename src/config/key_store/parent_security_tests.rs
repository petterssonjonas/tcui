use super::test_support::{env_lock, TestEnv};
use super::*;

#[cfg(unix)]
#[test]
fn explicit_legacy_import_rejects_symlink_file() {
    use std::os::unix::fs::symlink;
    let _guard = env_lock().lock().expect("env lock");
    let env = TestEnv::new("vault-legacy-file-symlink");
    let config = env.initialized_config();
    let path = PathBuf::from(config.key_file.as_deref().expect("path"));
    std::fs::create_dir_all(path.parent().expect("parent")).expect("mkdir");
    let target = path.with_file_name("target.toml");
    std::fs::write(&target, "[keys]\nOpenAI='do-not-import'\n").expect("target");
    symlink(&target, &path).expect("symlink");
    assert!(matches!(
        KeyStore::migrate_legacy(&config),
        Err(KeyStoreError::UnsafePath)
    ));
    assert!(!KeyStore::has_credential(&config, "OpenAI").expect("presence"));
    assert_eq!(
        std::fs::read_to_string(target).expect("target"),
        "[keys]\nOpenAI='do-not-import'\n"
    );
}

#[cfg(unix)]
#[test]
fn explicit_legacy_import_rejects_symlink_parent() {
    use std::os::unix::fs::symlink;
    let _guard = env_lock().lock().expect("env lock");
    let env = TestEnv::new("vault-legacy-parent-symlink");
    let mut config = env.initialized_config();
    let real = env.data_home().join("legacy-real");
    let link = env.data_home().join("legacy-link");
    std::fs::create_dir_all(&real).expect("mkdir");
    std::fs::write(real.join("keys.toml"), "[keys]\nOpenAI='do-not-import'\n").expect("target");
    symlink(&real, &link).expect("symlink");
    config.key_file = Some(link.join("keys.toml").display().to_string());
    assert!(matches!(
        KeyStore::migrate_legacy(&config),
        Err(KeyStoreError::UnsafePath)
    ));
    assert!(!KeyStore::has_credential(&config, "OpenAI").expect("presence"));
}
