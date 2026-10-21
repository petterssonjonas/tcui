use super::test_support::{env_lock, TestEnv};
use super::*;

#[cfg(unix)]
#[test]
fn vault_database_permissions_are_private() {
    use std::os::unix::fs::PermissionsExt;
    let _guard = env_lock().lock().expect("env lock");
    let env = TestEnv::new("vault-permissions");
    let config = env.initialized_config();
    KeyStore::set(&config, "OpenAI", "secret").expect("save");
    assert_eq!(
        std::fs::metadata(env.database_path())
            .expect("metadata")
            .permissions()
            .mode()
            & 0o777,
        0o600
    );
    assert_eq!(
        std::fs::metadata(env.database_path().parent().expect("parent"))
            .expect("metadata")
            .permissions()
            .mode()
            & 0o777,
        0o700
    );
}

#[cfg(unix)]
#[test]
fn vault_rejects_database_symlink_without_modifying_target() {
    use std::os::unix::fs::symlink;
    let _guard = env_lock().lock().expect("env lock");
    let env = TestEnv::new("vault-symlink");
    let config = env.config();
    let target = env.data_home().join("target.db");
    std::fs::create_dir_all(env.database_path().parent().expect("parent")).expect("mkdir");
    std::fs::write(&target, b"do not modify").expect("target");
    symlink(&target, env.database_path()).expect("symlink");
    assert!(matches!(
        KeyStore::vault_status(&config),
        Err(KeyStoreError::UnsafePath)
    ));
    assert_eq!(
        std::fs::read(target).expect("read target"),
        b"do not modify"
    );
}
