#![expect(
    clippy::await_holding_lock,
    reason = "Credential tests isolate process-global HOME and XDG paths."
)]

use chrono::{Duration, Utc};
use secrecy::ExposeSecret;

use super::resolver::resolve_provider_credential_with_config;
use super::resolver_tests::NativeTestEnvironment;
use super::{CredentialError, CredentialRequest, CredentialSource};
use crate::config::key_store::{
    ApiKeyCredential, ApiKeyCredentialOwnership, ApiKeyCredentialSource, Credential,
    OAuthCredential, OAuthCredentialOwnership, OAuthCredentialSource,
};
use crate::config::KeyStore;

#[test]
fn native_oauth_endpoints_are_pinned_but_explicit_api_key_endpoints_are_customizable() {
    for provider in [
        "Codex",
        "OpenAI OAuth",
        "OpenRouter OAuth",
        "Anthropic Agent SDK",
    ] {
        assert!(!super::trusted_provider_endpoint(
            provider,
            "https://custom.example/v1"
        ));
    }
    for provider in [
        "OpenAI",
        "Anthropic",
        "OpenRouter",
        "Gemini",
        "My exact endpoint",
    ] {
        assert!(super::trusted_provider_endpoint(
            provider,
            "https://custom.example/v1"
        ));
        assert!(!super::trusted_provider_endpoint(
            provider,
            "file:///private"
        ));
        assert!(!super::trusted_provider_endpoint(
            provider,
            "https://user:secret@custom.example/v1"
        ));
    }
    assert!(super::trusted_provider_endpoint(
        "Codex",
        "https://chatgpt.com/backend-api/codex"
    ));
    assert!(super::trusted_provider_endpoint(
        "OpenRouter OAuth",
        "https://openrouter.ai/api/v1"
    ));
}

#[tokio::test]
async fn resolver_ignores_environment_dotenv_and_external_logins_for_every_provider() {
    let _guard = crate::test_support::env_lock()
        .lock()
        .expect("environment lock");
    let environment = NativeTestEnvironment::new("no-credential-discovery");
    let original = std::env::var_os("TCUI_EXPLICIT_CONNECTION_TEST_KEY");
    std::env::set_var("TCUI_EXPLICIT_CONNECTION_TEST_KEY", "environment-secret");
    let home = dirs::home_dir().unwrap();
    std::fs::write(
        home.join(".env"),
        "TCUI_EXPLICIT_CONNECTION_TEST_KEY=dotenv-secret\n",
    )
    .unwrap();
    for directory in [".codex", ".gemini"] {
        std::fs::create_dir_all(home.join(directory)).unwrap();
        for file in ["auth.json", "oauth_creds.json"] {
            std::fs::write(home.join(directory).join(file), r#"{"access_token":"external-secret","tokens":{"access_token":"external-secret","account_id":"account"}}"#).unwrap();
        }
    }
    for (provider, endpoint) in [
        ("OpenAI", "https://api.openai.com/v1"),
        ("OpenRouter", "https://openrouter.ai/api/v1"),
        ("OpenRouter OAuth", "https://openrouter.ai/api/v1"),
        ("Gemini", "https://generativelanguage.googleapis.com/v1beta"),
        ("Codex", "https://chatgpt.com/backend-api/codex"),
        ("Anthropic Agent SDK", "https://api.anthropic.com/v1"),
    ] {
        let result = resolve_provider_credential_with_config(
            CredentialRequest::new(provider, "TCUI_EXPLICIT_CONNECTION_TEST_KEY", endpoint),
            &environment.config(),
        )
        .await
        .unwrap();
        assert!(
            result.is_none(),
            "unexpected discovered credential for {provider}"
        );
    }
    match original {
        Some(value) => std::env::set_var("TCUI_EXPLICIT_CONNECTION_TEST_KEY", value),
        None => std::env::remove_var("TCUI_EXPLICIT_CONNECTION_TEST_KEY"),
    }
}

#[tokio::test]
async fn saved_keys_keep_exact_connection_identity_and_allow_custom_endpoints() {
    let _guard = crate::test_support::env_lock()
        .lock()
        .expect("environment lock");
    let environment = NativeTestEnvironment::new("exact-connection-keys");
    let config = environment.config();
    for name in ["OpenAI", "my custom Connection", "Gemini"] {
        KeyStore::set(&config, name, "saved-key").unwrap();
        let credential = resolve_provider_credential_with_config(
            CredentialRequest::new(name, "IGNORED", "https://custom.example/v1"),
            &config,
        )
        .await
        .unwrap()
        .unwrap();
        assert_eq!(credential.source(), CredentialSource::TcuiStoredApiKey);
        assert_eq!(credential.bearer_token().expose_secret(), "saved-key");
    }
}

#[tokio::test]
async fn openrouter_key_and_oauth_connections_never_shadow_each_other() {
    let _guard = crate::test_support::env_lock()
        .lock()
        .expect("environment lock");
    let environment = NativeTestEnvironment::new("openrouter-separate-connections");
    let config = environment.config();
    KeyStore::set(&config, "OpenRouter", "manual-key").unwrap();
    KeyStore::upsert_credential(
        &config,
        &Credential::ApiKey(
            ApiKeyCredential::new(
                "OpenRouter OAuth",
                "pkce-key",
                ApiKeyCredentialOwnership::Tcui,
                ApiKeyCredentialSource::OpenRouterPkce,
            )
            .unwrap(),
        ),
    )
    .unwrap();
    for (provider, expected) in [
        ("OpenRouter", "manual-key"),
        ("OpenRouter OAuth", "pkce-key"),
    ] {
        let credential = resolve_provider_credential_with_config(
            CredentialRequest::new(provider, "IGNORED", "https://openrouter.ai/api/v1"),
            &config,
        )
        .await
        .unwrap()
        .unwrap();
        assert_eq!(credential.bearer_token().expose_secret(), expected);
    }
    assert!(matches!(
        resolve_provider_credential_with_config(
            CredentialRequest::new("OpenRouter OAuth", "IGNORED", "https://custom.example/v1"),
            &config,
        )
        .await,
        Err(CredentialError::UntrustedEndpoint)
    ));
}

#[tokio::test]
async fn resolver_tags_native_oauth_and_reports_locked_store_actionably() {
    let _guard = crate::test_support::env_lock()
        .lock()
        .expect("environment lock");
    let environment = NativeTestEnvironment::new("native-connection-source");
    let config = environment.config();
    KeyStore::upsert_oauth(
        &config,
        &OAuthCredential {
            provider: "Codex".to_owned(),
            access_token: "native-token".to_owned(),
            refresh_token: Some("refresh-token".to_owned()),
            expires_at: Utc::now() + Duration::hours(1),
            account_id: Some("account".to_owned()),
            ownership: OAuthCredentialOwnership::Tcui,
            source: OAuthCredentialSource::NativeOAuth,
        },
    )
    .unwrap();
    let request = || {
        CredentialRequest::new(
            "OpenAI OAuth",
            "IGNORED",
            "https://chatgpt.com/backend-api/codex",
        )
    };
    let credential = resolve_provider_credential_with_config(request(), &config)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(credential.source(), CredentialSource::TcuiNativeOAuth);
    KeyStore::lock_vault(&config).unwrap();
    let error = resolve_provider_credential_with_config(request(), &config)
        .await
        .unwrap_err();
    assert_eq!(error, CredentialError::CredentialStore);
    assert!(error.to_string().contains("/vault"));
}
