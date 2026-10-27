#![expect(
    clippy::await_holding_lock,
    reason = "Resolver fixtures isolate process-global HOME and XDG paths across async checks."
)]

use super::{CredentialError, CredentialRequest};
use std::ffi::OsString;
use std::path::PathBuf;

use crate::config::AppConfig;
use crate::llm::model_fetcher::fetch_models;

struct TestEnvironment {
    root: PathBuf,
    original_home: Option<OsString>,
    original_data_home: Option<OsString>,
}

impl TestEnvironment {
    fn new(label: &str) -> Self {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("system clock")
            .as_nanos();
        let root = std::env::temp_dir().join(format!("tcui-{label}-{nanos}"));
        std::fs::create_dir_all(&root).expect("create test root");
        let original_home = std::env::var_os("HOME");
        let original_data_home = std::env::var_os("XDG_DATA_HOME");
        std::env::set_var("HOME", &root);
        std::env::set_var("XDG_DATA_HOME", root.join("data"));
        Self {
            root,
            original_home,
            original_data_home,
        }
    }

    fn config(&self) -> AppConfig {
        AppConfig {
            key_file: Some(self.root.join("keys.toml").display().to_string()),
            ..AppConfig::default()
        }
    }

    fn write_external_codex_auth(&self) {
        let auth_dir = self.root.join(".codex");
        std::fs::create_dir_all(&auth_dir).expect("create Codex auth directory");
        let auth_file = auth_dir.join("auth.json");
        std::fs::write(
            &auth_file,
            r#"{"tokens":{"access_token":"external-access","account_id":"external-account"}}"#,
        )
        .expect("write external Codex credential");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;

            std::fs::set_permissions(&auth_file, std::fs::Permissions::from_mode(0o600))
                .expect("restrict external Codex credential");
        }
    }
}

impl Drop for TestEnvironment {
    fn drop(&mut self) {
        match self.original_home.take() {
            Some(home) => std::env::set_var("HOME", home),
            None => std::env::remove_var("HOME"),
        }
        match self.original_data_home.take() {
            Some(data_home) => std::env::set_var("XDG_DATA_HOME", data_home),
            None => std::env::remove_var("XDG_DATA_HOME"),
        }
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

#[tokio::test]
async fn resolver_rejects_a_corrupt_native_store_without_falling_back_to_external_codex() {
    // Given
    let _guard = crate::test_support::env_lock()
        .lock()
        .expect("environment lock");
    let environment = TestEnvironment::new("resolver-corrupt-store");
    let config = environment.config();
    std::fs::write(
        config.key_file.as_ref().expect("key file"),
        "not valid toml",
    )
    .expect("corrupt native store");
    environment.write_external_codex_auth();
    let request = CredentialRequest::new(
        "Codex",
        "OPENAI_API_KEY",
        "https://chatgpt.com/backend-api/codex",
    );

    // When
    let result = super::resolver::resolve_provider_credential_with_config(request, &config).await;

    // Then
    assert!(matches!(
        result,
        Err(CredentialError::CodexCredentialUnavailable)
    ));
}

#[tokio::test]
async fn model_fetcher_rejects_an_api_key_when_codex_requires_oauth() {
    // Given
    let credential = super::Credential::api_key_for_test("OpenRouter", "wrong-key-type");

    // When
    let models = fetch_models(
        "Codex",
        "https://chatgpt.com/backend-api/codex",
        Some(&credential),
        "codex",
    )
    .await;

    // Then
    assert!(models.is_empty());
}
