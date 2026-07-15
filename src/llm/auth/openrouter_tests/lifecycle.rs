use std::ffi::OsString;
use std::path::PathBuf;
use std::time::Duration;

use crate::config::key_store::{
    ApiKeyCredential, ApiKeyCredentialOwnership, ApiKeyCredentialSource, Credential,
};
use crate::config::{AppConfig, KeyStore};
use crate::llm::auth::{
    oauth::{oauth_cancellation, RedirectUri},
    read_provider_api_key,
};
use crate::storage::Storage;

use super::fixture::{body, fixture, response};

pub(super) struct TestEnv {
    root: PathBuf,
    config_home: Option<OsString>,
    data_home: Option<OsString>,
    openrouter_api_key: Option<OsString>,
}

impl TestEnv {
    pub(super) fn new(label: &str) -> Self {
        let root = std::env::temp_dir().join(format!("tcui-{label}-{}", rand::random::<u64>()));
        std::fs::create_dir_all(root.join("config")).expect("create config root");
        std::fs::create_dir_all(root.join("data")).expect("create data root");
        let config_home = std::env::var_os("XDG_CONFIG_HOME");
        let data_home = std::env::var_os("XDG_DATA_HOME");
        let openrouter_api_key = std::env::var_os("OPENROUTER_API_KEY");
        std::env::set_var("XDG_CONFIG_HOME", root.join("config"));
        std::env::set_var("XDG_DATA_HOME", root.join("data"));
        std::env::remove_var("OPENROUTER_API_KEY");
        Self {
            root,
            config_home,
            data_home,
            openrouter_api_key,
        }
    }

    pub(super) fn key_file(&self) -> PathBuf {
        self.root.join("data").join("tcui").join("keys.toml")
    }
}

impl Drop for TestEnv {
    fn drop(&mut self) {
        restore_env("XDG_CONFIG_HOME", self.config_home.take());
        restore_env("XDG_DATA_HOME", self.data_home.take());
        restore_env("OPENROUTER_API_KEY", self.openrouter_api_key.take());
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

fn restore_env(name: &str, value: Option<OsString>) {
    match value {
        Some(value) => std::env::set_var(name, value),
        None => std::env::remove_var(name),
    }
}

fn local_credential(value: &str) -> Credential {
    Credential::ApiKey(
        ApiKeyCredential::new(
            "OpenRouter",
            value,
            ApiKeyCredentialOwnership::Tcui,
            ApiKeyCredentialSource::OpenRouterPkce,
        )
        .expect("valid local credential"),
    )
}

#[tokio::test]
#[expect(
    clippy::await_holding_lock,
    reason = "The process-wide environment fixture must remain isolated through async cleanup."
)]
async fn successful_exchange_replaces_prior_key_without_serializing_either_secret(
) -> Result<(), Box<dyn std::error::Error>> {
    // Given
    let _guard = crate::test_support::env_lock()
        .lock()
        .expect("env lock poisoned");
    let env = TestEnv::new("openrouter-replacement");
    let fixture = fixture(
        vec![response("200 OK", r#"{"key":"replacement-key-canary"}"#)],
        Duration::from_secs(1),
    )
    .await?;
    let config = AppConfig::default();
    KeyStore::upsert_credential(&config, &local_credential("prior-key-canary"))?;
    let prior_bytes = std::fs::read(env.key_file())?;
    let (cancellation, _) = oauth_cancellation();

    // When
    fixture
        .adapter
        .exchange_and_persist(
            &config,
            fixture
                .adapter
                .begin_headless(RedirectUri::parse("http://127.0.0.1:7777/callback")?)?
                .complete_headless(&mut super::contract::PastedInput::code("replacement-code"))?,
            &cancellation,
        )
        .await?;
    let replacement_bytes = std::fs::read(env.key_file())?;
    let raw = std::str::from_utf8(&replacement_bytes)?;
    let resolved = read_provider_api_key("OpenRouter", "OPENROUTER_API_KEY", &Storage::new()?);
    let stored = KeyStore::get_credential(&config, "OpenRouter")?;
    let _ = fixture.finish().await?;

    // Then
    assert_ne!(prior_bytes, replacement_bytes);
    assert!(!raw.contains("prior-key-canary") && !raw.contains("replacement-key-canary"));
    assert_eq!(resolved.as_deref(), Some("replacement-key-canary"));
    assert_eq!(stored, Some(local_credential("replacement-key-canary")));
    Ok(())
}

#[tokio::test]
#[expect(
    clippy::await_holding_lock,
    reason = "The process-wide environment fixture must remain isolated through async cleanup."
)]
async fn documented_code_creation_exchange_resolve_and_local_logout_round_trip(
) -> Result<(), Box<dyn std::error::Error>> {
    // Given
    let _guard = crate::test_support::env_lock()
        .lock()
        .expect("env lock poisoned");
    let _env = TestEnv::new("openrouter-lifecycle");
    let fixture = fixture(
        vec![
            response(
                "200 OK",
                r#"{"data":{"id":"created-code","extension":"ignored"}}"#,
            ),
            response(
                "200 OK",
                r#"{"key":"exchanged-key","user_id":"user-1","extension":"ignored"}"#,
            ),
        ],
        Duration::from_secs(1),
    )
    .await?;
    let config = AppConfig::default();
    let (cancellation, _) = oauth_cancellation();
    let authorization = fixture
        .adapter
        .begin_headless(RedirectUri::parse("http://127.0.0.1:7777/callback")?)?;

    // When
    let grant = fixture
        .adapter
        .create_authorization_code("management-key", authorization, &cancellation)
        .await?;
    fixture
        .adapter
        .exchange_and_persist(&config, grant, &cancellation)
        .await?;
    let storage = Storage::new()?;
    let resolved = read_provider_api_key("OpenRouter", "OPENROUTER_API_KEY", &storage);
    let stored = KeyStore::get_credential(&config, "OpenRouter")?;
    let removed = crate::llm::auth::openrouter::OpenRouterAdapter::logout(&config)?;
    let removed_again = crate::llm::auth::openrouter::OpenRouterAdapter::logout(&config)?;
    let requests = fixture.finish().await?;

    // Then
    let create: serde_json::Value = serde_json::from_str(body(&requests[0]))?;
    let exchange: serde_json::Value = serde_json::from_str(body(&requests[1]))?;
    assert!(requests[0].starts_with("POST /auth/keys/code HTTP/1.1"));
    assert!(requests[0]
        .to_ascii_lowercase()
        .contains("authorization: bearer management-key"));
    assert_eq!(create["callback_url"], "http://127.0.0.1:7777/callback");
    assert_eq!(create["code_challenge_method"], "S256");
    assert_eq!(exchange["code"], "created-code");
    assert_eq!(exchange["code_challenge_method"], "S256");
    assert!(!requests[1].to_ascii_lowercase().contains("authorization:"));
    assert_eq!(resolved.as_deref(), Some("exchanged-key"));
    assert_eq!(
        stored.as_ref().map(Credential::as_api_key),
        Some("exchanged-key")
    );
    assert!(removed && !removed_again);
    Ok(())
}
