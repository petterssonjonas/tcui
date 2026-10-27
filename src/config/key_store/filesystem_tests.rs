use super::persistence::{
    inject_cleanup_directory_clone_failure, write_file, write_file_with_before_rename,
};
#[cfg(unix)]
use super::rollback::{
    inject_commit_backup_remove_failure, inject_persistent_restore_backup_rename_failure,
    inject_restore_backup_rename_failure, inject_restore_target_remove_failure, PreviousFileGuard,
};
use super::test_support::{env_lock, TestEnv};
use super::*;
use std::collections::BTreeMap;
use std::path::PathBuf;

fn key_path(config: &AppConfig) -> PathBuf {
    PathBuf::from(config.key_file.as_deref().expect("key file path"))
}

#[cfg(unix)]
fn replace_target_atomically(path: &std::path::Path, contents: &[u8]) {
    let replacement = path.with_file_name("replacement-keys.toml");
    std::fs::write(&replacement, contents).expect("write replacement credential file");
    std::fs::rename(replacement, path).expect("replace credential file");
}

#[cfg(unix)]
fn capture_previous_file_guard(path: &std::path::Path) -> PreviousFileGuard {
    let parent = path.parent().expect("key file parent");
    let directory = cap_std::fs::Dir::open_ambient_dir(parent, cap_std::ambient_authority())
        .expect("open key file parent");
    PreviousFileGuard::capture(&directory, path.file_name().expect("key file name"))
        .expect("capture previous credential file")
}

#[cfg(unix)]
fn has_no_key_artifacts(parent: &std::path::Path) -> bool {
    std::fs::read_dir(parent)
        .expect("read key file parent")
        .all(|entry| {
            !entry
                .expect("read key file parent entry")
                .file_name()
                .to_string_lossy()
                .starts_with(".tcui-keys-")
        })
}

#[cfg(unix)]
fn preserved_backup(parent: &std::path::Path) -> Option<PathBuf> {
    std::fs::read_dir(parent)
        .expect("read key file parent")
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .find(|path| {
            path.file_name()
                .is_some_and(|name| name.to_string_lossy().starts_with(".tcui-keys-backup-"))
        })
}

#[cfg(unix)]
#[test]
fn write_rejects_key_file_replaced_with_symlink_before_atomic_rename() {
    use std::os::unix::fs::symlink;

    let _guard = env_lock().lock().expect("env lock poisoned");
    let env = TestEnv::new("oauth-symlink-before-rename");
    let config = env.config();
    let path = key_path(&config);
    let target = path.with_file_name("symlink-target.toml");
    let original = "unrelated-secret-target";
    std::fs::create_dir_all(path.parent().expect("key file parent"))
        .expect("create key file parent");
    std::fs::write(&target, original).expect("write symlink target");
    let replacement = StoredKeysFile {
        version: 1,
        keys: BTreeMap::new(),
        oauth: BTreeMap::new(),
        credentials: BTreeMap::new(),
    };

    let error = write_file_with_before_rename(&path, &replacement, || {
        symlink(&target, &path).expect("replace key file with symlink");
    })
    .expect_err("symlink replacement must fail");

    assert!(matches!(error, KeyStoreError::UnsafePath));
    assert!(std::fs::symlink_metadata(&path)
        .expect("inspect replacement key path")
        .file_type()
        .is_symlink());
    assert_eq!(
        std::fs::read_to_string(target).expect("read symlink target"),
        original
    );
}

#[cfg(unix)]
#[test]
fn write_rejects_parent_replacement_without_clobbering_replacement_target() {
    use std::os::unix::fs::symlink;

    let _guard = env_lock().lock().expect("env lock poisoned");
    let env = TestEnv::new("oauth-parent-replacement");
    let config = env.config();
    let path = key_path(&config);
    let parent = path.parent().expect("key file parent").to_path_buf();
    let archived_parent = parent.with_file_name("parent-before-replacement");
    let replacement_target = env.data_home().join("replacement-target");
    std::fs::create_dir_all(&parent).expect("create key file parent");
    std::fs::create_dir_all(&replacement_target).expect("create replacement target");
    let replacement = StoredKeysFile {
        version: 1,
        keys: BTreeMap::new(),
        oauth: BTreeMap::new(),
        credentials: BTreeMap::new(),
    };

    let error = write_file_with_before_rename(&path, &replacement, || {
        std::fs::rename(&parent, &archived_parent).expect("move original parent");
        symlink(&replacement_target, &parent).expect("replace parent with symlink");
    })
    .expect_err("parent replacement must fail");

    assert!(matches!(error, KeyStoreError::ParentChanged));
    assert!(!replacement_target.join("keys.toml").exists());
    assert!(
        std::fs::read_dir(archived_parent)
            .expect("read archived original parent")
            .all(|entry| {
                !entry
                    .expect("read archived directory entry")
                    .file_name()
                    .to_string_lossy()
                    .starts_with(".tcui-keys-")
            }),
        "parent replacement must clean the temporary file through the held directory"
    );
}

#[cfg(unix)]
#[test]
fn write_removes_temporary_artifact_when_cleanup_directory_clone_fails() {
    let _guard = env_lock().lock().expect("env lock poisoned");
    let env = TestEnv::new("oauth-cleanup-directory-clone-failure");
    let config = env.config();
    let path = key_path(&config);
    let parent = path.parent().expect("key file parent");
    let original = b"encrypted-original-credential-bytes";
    std::fs::create_dir_all(parent).expect("create key file parent");
    std::fs::write(&path, original).expect("write original credential file");
    let replacement = StoredKeysFile {
        version: 1,
        keys: BTreeMap::new(),
        oauth: BTreeMap::new(),
        credentials: BTreeMap::new(),
    };

    inject_cleanup_directory_clone_failure();
    let error = write_file(&path, &replacement)
        .expect_err("injected cleanup-directory clone failure must fail write");

    assert!(matches!(error, KeyStoreError::Write));
    assert_eq!(
        std::fs::read(&path).expect("read original credential file"),
        original
    );
    assert!(has_no_key_artifacts(parent));
}

#[cfg(unix)]
#[test]
fn restore_retries_backup_rename_after_initial_failure() {
    let _guard = env_lock().lock().expect("env lock poisoned");
    let env = TestEnv::new("oauth-restore-backup-rename-failure");
    let path = key_path(&env.config());
    let parent = path.parent().expect("key file parent");
    let original = b"encrypted-original-credential-bytes";
    std::fs::create_dir_all(parent).expect("create key file parent");
    std::fs::write(&path, original).expect("write original credential file");
    let mut rollback = capture_previous_file_guard(&path);
    replace_target_atomically(&path, b"encrypted-replacement-credential-bytes");
    rollback.mark_replaced();

    inject_restore_backup_rename_failure();
    let error = rollback
        .restore()
        .expect_err("injected backup rename failure must fail restore");

    assert!(matches!(error, KeyStoreError::Write));
    drop(rollback);
    assert!(has_no_key_artifacts(parent));
    assert_eq!(
        std::fs::read(&path).expect("read restored credential file"),
        original
    );
}

#[cfg(unix)]
#[test]
fn drop_preserves_backup_when_restore_rename_fails_persistently() {
    use std::os::unix::fs::PermissionsExt;

    let _guard = env_lock().lock().expect("env lock poisoned");
    let env = TestEnv::new("oauth-persistent-restore-rename-failure");
    let path = key_path(&env.config());
    let parent = path.parent().expect("key file parent");
    let original = b"encrypted-original-credential-bytes";
    let replacement = b"encrypted-replacement-credential-bytes";
    std::fs::create_dir_all(parent).expect("create key file parent");
    std::fs::write(&path, original).expect("write original credential file");
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600))
        .expect("restrict original credential file mode");
    let mut rollback = capture_previous_file_guard(&path);
    replace_target_atomically(&path, replacement);
    rollback.mark_replaced();

    let persistent_failure = inject_persistent_restore_backup_rename_failure();
    let error = rollback
        .restore()
        .expect_err("persistent backup rename failure must fail restore");

    assert!(matches!(error, KeyStoreError::Write));
    drop(rollback);
    drop(persistent_failure);
    let backup = preserved_backup(parent).expect("persistent failure must preserve backup");
    assert_eq!(
        std::fs::read(&backup).expect("read preserved backup"),
        original
    );
    assert_eq!(
        std::fs::read(&path).expect("read replacement credential file"),
        replacement
    );
    assert_eq!(
        std::fs::metadata(&backup)
            .expect("read backup metadata")
            .permissions()
            .mode()
            & 0o777,
        0o600
    );
}

#[cfg(unix)]
#[test]
fn commit_retries_backup_removal_after_initial_failure() {
    let _guard = env_lock().lock().expect("env lock poisoned");
    let env = TestEnv::new("oauth-commit-backup-remove-failure");
    let path = key_path(&env.config());
    let parent = path.parent().expect("key file parent");
    let original = b"encrypted-original-credential-bytes";
    std::fs::create_dir_all(parent).expect("create key file parent");
    std::fs::write(&path, original).expect("write original credential file");
    let mut rollback = capture_previous_file_guard(&path);
    replace_target_atomically(&path, b"encrypted-replacement-credential-bytes");
    rollback.mark_replaced();

    inject_commit_backup_remove_failure();
    let error = rollback
        .commit()
        .expect_err("injected backup removal failure must fail commit");

    assert!(matches!(error, KeyStoreError::Write));
    drop(rollback);
    assert!(has_no_key_artifacts(parent));
    assert_eq!(
        std::fs::read(&path).expect("read restored credential file"),
        original
    );
}

#[cfg(unix)]
#[test]
fn restore_retries_new_target_removal_after_initial_failure() {
    let _guard = env_lock().lock().expect("env lock poisoned");
    let env = TestEnv::new("oauth-restore-target-remove-failure");
    let path = key_path(&env.config());
    let parent = path.parent().expect("key file parent");
    std::fs::create_dir_all(parent).expect("create key file parent");
    let mut rollback = capture_previous_file_guard(&path);
    std::fs::write(&path, b"encrypted-new-credential-bytes").expect("write new credential file");
    rollback.mark_replaced();

    inject_restore_target_remove_failure();
    let error = rollback
        .restore()
        .expect_err("injected target removal failure must fail restore");

    assert!(matches!(error, KeyStoreError::Write));
    drop(rollback);
    assert!(!path.exists());
    assert!(has_no_key_artifacts(parent));
}
