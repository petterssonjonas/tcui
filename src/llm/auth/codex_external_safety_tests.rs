#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;

use super::codex::{external_metadata_is_safe, read_external_credential, CodexCredentialError};
use super::codex_test_support::TestEnvironment;

#[cfg(unix)]
#[test]
fn external_auth_accepts_owner_only_file_without_mutation() -> Result<(), Box<dyn std::error::Error>>
{
    let _guard = crate::test_support::env_lock()
        .lock()
        .expect("environment lock poisoned");
    let environment = TestEnvironment::new("codex-external-safe-mode")?;
    let contents = r#"{"tokens":{"access_token":"external-access"}}"#;
    environment.write_external_auth(contents)?;
    let mut permissions = std::fs::metadata(environment.auth_path())?.permissions();
    permissions.set_mode(0o600);
    std::fs::set_permissions(environment.auth_path(), permissions)?;

    let credential = read_external_credential()?;

    assert!(credential.is_some());
    assert_eq!(std::fs::read_to_string(environment.auth_path())?, contents);
    Ok(())
}

#[cfg(unix)]
#[test]
fn external_auth_rejects_group_readable_file_without_mutation(
) -> Result<(), Box<dyn std::error::Error>> {
    let _guard = crate::test_support::env_lock()
        .lock()
        .expect("environment lock poisoned");
    let environment = TestEnvironment::new("codex-external-unsafe-mode")?;
    let contents = r#"{"tokens":{"access_token":"external-access"}}"#;
    environment.write_external_auth(contents)?;
    let mut permissions = std::fs::metadata(environment.auth_path())?.permissions();
    permissions.set_mode(0o640);
    std::fs::set_permissions(environment.auth_path(), permissions)?;

    let error = read_external_credential().unwrap_err();

    assert!(matches!(error, CodexCredentialError::UnsafeFile));
    assert_eq!(std::fs::read_to_string(environment.auth_path())?, contents);
    Ok(())
}

#[test]
fn external_auth_metadata_rejects_non_owner_and_group_or_world_permissions() {
    assert!(external_metadata_is_safe(1000, 0o600, 1000));
    assert!(!external_metadata_is_safe(1000, 0o640, 1000));
    assert!(!external_metadata_is_safe(1000, 0o604, 1000));
    assert!(!external_metadata_is_safe(1001, 0o600, 1000));
}
