use super::codex::{codex_status, resolve_credential, CodexStatus};
use super::codex_test_support::TestEnvironment;
use crate::config::AppConfig;

#[test]
fn external_codex_login_is_never_a_tcui_connection() -> Result<(), Box<dyn std::error::Error>> {
    let _guard = crate::test_support::env_lock().lock().unwrap();
    let environment = TestEnvironment::new("codex-no-passive-login")?;
    let original =
        r#"{"tokens":{"access_token":"external-secret","account_id":"external-account"}}"#;
    environment.write_external_auth(original)?;
    assert!(resolve_credential(&AppConfig::default())?.is_none());
    assert!(matches!(
        codex_status(&AppConfig::default())?,
        CodexStatus::Unauthenticated
    ));
    assert_eq!(std::fs::read_to_string(environment.auth_path())?, original);
    Ok(())
}
