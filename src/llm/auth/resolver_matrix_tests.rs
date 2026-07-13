#![expect(
    clippy::await_holding_lock,
    reason = "Resolver fixtures isolate process-global HOME and XDG paths across async checks."
)]

// allow: SIZE_OK — the complete credential-source and endpoint-trust matrix is reviewed together.

use std::ffi::OsString;

use chrono::{Duration, Utc};

use super::resolver::resolve_provider_credential_with_config;
use super::resolver_tests::NativeTestEnvironment;
use super::{CredentialError, CredentialRequest, CredentialSource};
use crate::config::KeyStore;
use crate::config::key_store::{
    ApiKeyCredential, ApiKeyCredentialOwnership, ApiKeyCredentialSource, Credential,
    OAuthCredential, OAuthCredentialOwnership, OAuthCredentialSource,
};

const NAMED_PROVIDER_ENDPOINTS: &[(&str, &str)] = &[
    ("Codex", "https://chatgpt.com/backend-api/codex"),
    ("Gemini", "https://generativelanguage.googleapis.com/v1beta"),
    ("OpenAI", "https://api.openai.com/v1"),
    ("Anthropic", "https://api.anthropic.com/v1"),
    ("Ollama", "http://localhost:11434/v1"),
    ("OpenRouter", "https://openrouter.ai/api/v1"),
    ("Kilo Gateway", "https://api.kilo.ai/api/gateway"),
    ("Mistral", "https://api.mistral.ai/v1"),
    ("Groq", "https://api.groq.com/openai/v1"),
    ("Berget.ai", "https://api.berget.ai/v1"),
    ("OpenCode Go", "https://opencode.ai/zen/go/v1"),
    ("OpenCode Zen", "https://opencode.ai/zen/v1"),
];

struct EnvVarGuard {
    name: &'static str,
    original: Option<OsString>,
}

impl EnvVarGuard {
    fn replace(name: &'static str, value: &str) -> Self {
        let original = std::env::var_os(name);
        std::env::set_var(name, value);
        Self { name, original }
    }
}

impl Drop for EnvVarGuard {
    fn drop(&mut self) {
        match self.original.take() {
            Some(value) => std::env::set_var(self.name, value),
            None => std::env::remove_var(self.name),
        }
    }
}

#[test]
fn every_named_provider_accepts_only_its_trusted_endpoint() {
    for (provider, endpoint) in NAMED_PROVIDER_ENDPOINTS {
        assert!(super::trusted_provider_endpoint(provider, endpoint));
        assert!(!super::trusted_provider_endpoint(
            provider,
            "https://untrusted.example/v1"
        ));
    }
}

#[tokio::test]
async fn resolver_rejects_every_named_provider_before_reading_environment_credentials() {
    let _guard = crate::test_support::env_lock()
        .lock()
        .expect("environment lock");
    let environment = NativeTestEnvironment::new("resolver-untrusted-provider-matrix");
    let config = environment.config();
    let _key = EnvVarGuard::replace("TCUI_TODO8_MATRIX_KEY", "environment-secret");

    for (provider, _) in NAMED_PROVIDER_ENDPOINTS {
        let result = resolve_provider_credential_with_config(
            CredentialRequest::new(
                provider,
                "TCUI_TODO8_MATRIX_KEY",
                "https://untrusted.example/v1",
            ),
            &config,
        )
        .await;

        assert!(matches!(result, Err(CredentialError::UntrustedEndpoint)));
    }
}

#[tokio::test]
async fn resolver_tags_environment_credentials() {
    let _guard = crate::test_support::env_lock()
        .lock()
        .expect("environment lock");
    let environment = NativeTestEnvironment::new("resolver-environment-source");
    let _key = EnvVarGuard::replace("TCUI_TODO8_ENV_KEY", "environment-key");

    let credential = resolve_provider_credential_with_config(
        CredentialRequest::new("OpenAI", "TCUI_TODO8_ENV_KEY", "https://api.openai.com/v1"),
        &environment.config(),
    )
    .await
    .expect("resolve environment credential")
    .expect("environment credential");

    assert_eq!(credential.source(), CredentialSource::Environment);
}

#[tokio::test]
async fn resolver_tags_dotenv_credentials() {
    let _guard = crate::test_support::env_lock()
        .lock()
        .expect("environment lock");
    let environment = NativeTestEnvironment::new("resolver-dotenv-source");
    let home = dirs::home_dir().expect("test home");
    std::fs::write(home.join(".env"), "TCUI_TODO8_DOTENV_KEY=dotenv-key\n")
        .expect("write dotenv fixture");

    let credential = resolve_provider_credential_with_config(
        CredentialRequest::new(
            "OpenAI",
            "TCUI_TODO8_DOTENV_KEY",
            "https://api.openai.com/v1",
        ),
        &environment.config(),
    )
    .await
    .expect("resolve dotenv credential")
    .expect("dotenv credential");

    assert_eq!(credential.source(), CredentialSource::DotEnv);
}

#[tokio::test]
async fn resolver_tags_legacy_key_store_credentials() {
    let _guard = crate::test_support::env_lock()
        .lock()
        .expect("environment lock");
    let environment = NativeTestEnvironment::new("resolver-legacy-key-store-source");
    let config = environment.config();
    KeyStore::save_keys(&config, &[("OpenAI".to_string(), "legacy-key".to_string())])
        .expect("store legacy key");

    let credential = resolve_provider_credential_with_config(
        CredentialRequest::new(
            "OpenAI",
            "TCUI_TODO8_LEGACY_KEY",
            "https://api.openai.com/v1",
        ),
        &config,
    )
    .await
    .expect("resolve legacy credential")
    .expect("legacy credential");

    assert_eq!(credential.source(), CredentialSource::LegacyKeyStore);
}

#[tokio::test]
async fn resolver_tags_tcui_stored_api_key_credentials() {
    let _guard = crate::test_support::env_lock()
        .lock()
        .expect("environment lock");
    let environment = NativeTestEnvironment::new("resolver-stored-api-key-source");
    let config = environment.config();
    let credential = ApiKeyCredential::new(
        "OpenRouter",
        "stored-key",
        ApiKeyCredentialOwnership::Tcui,
        ApiKeyCredentialSource::OpenRouterPkce,
    )
    .expect("create stored API key credential");
    KeyStore::upsert_credential(&config, &Credential::ApiKey(credential))
        .expect("store API key credential");

    let credential = resolve_provider_credential_with_config(
        CredentialRequest::new(
            "OpenRouter",
            "TCUI_TODO8_STORED_KEY",
            "https://openrouter.ai/api/v1",
        ),
        &config,
    )
    .await
    .expect("resolve stored API key credential")
    .expect("stored API key credential");

    assert_eq!(credential.source(), CredentialSource::TcuiStoredApiKey);
}

#[tokio::test]
async fn resolver_tags_passive_external_gemini_tokens() {
    let _guard = crate::test_support::env_lock()
        .lock()
        .expect("environment lock");
    let environment = NativeTestEnvironment::new("resolver-passive-gemini-source");
    let gemini_dir = dirs::home_dir().expect("test home").join(".gemini");
    std::fs::create_dir_all(&gemini_dir).expect("create Gemini fixture directory");
    std::fs::write(
        gemini_dir.join("oauth_creds.json"),
        r#"{"access_token":"passive-gemini-token"}"#,
    )
    .expect("write Gemini fixture");

    let credential = resolve_provider_credential_with_config(
        CredentialRequest::new(
            "Gemini",
            "TCUI_TODO8_GEMINI_KEY",
            "https://generativelanguage.googleapis.com/v1beta",
        ),
        &environment.config(),
    )
    .await
    .expect("resolve passive Gemini token")
    .expect("passive Gemini token");

    assert_eq!(credential.source(), CredentialSource::PassiveExternalToken);
}

#[tokio::test]
async fn resolver_tags_external_codex_cli_credentials() {
    let _guard = crate::test_support::env_lock()
        .lock()
        .expect("environment lock");
    let environment = NativeTestEnvironment::new("resolver-external-codex-source");
    let auth_dir = dirs::home_dir().expect("test home").join(".codex");
    std::fs::create_dir_all(&auth_dir).expect("create Codex fixture directory");
    let auth_file = auth_dir.join("auth.json");
    std::fs::write(
        &auth_file,
        r#"{"tokens":{"access_token":"external-codex-token","account_id":"account-123"}}"#,
    )
    .expect("write Codex fixture");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;

        std::fs::set_permissions(&auth_file, std::fs::Permissions::from_mode(0o600))
            .expect("restrict Codex fixture");
    }

    let credential = resolve_provider_credential_with_config(
        CredentialRequest::new(
            "Codex",
            "TCUI_TODO8_CODEX_KEY",
            "https://chatgpt.com/backend-api/codex",
        ),
        &environment.config(),
    )
    .await
    .expect("resolve external Codex credential")
    .expect("external Codex credential");

    assert_eq!(credential.source(), CredentialSource::ExternalCodexCli);
}

#[tokio::test]
async fn resolver_tags_tcui_native_codex_credentials() {
    let _guard = crate::test_support::env_lock()
        .lock()
        .expect("environment lock");
    let environment = NativeTestEnvironment::new("resolver-native-codex-source");
    let config = environment.config();
    KeyStore::upsert_oauth(
        &config,
        &OAuthCredential {
            provider: "Codex".to_string(),
            access_token: "native-codex-token".to_string(),
            refresh_token: Some("native-refresh-token".to_string()),
            expires_at: Utc::now() + Duration::hours(1),
            account_id: Some("account-123".to_string()),
            ownership: OAuthCredentialOwnership::Tcui,
            source: OAuthCredentialSource::NativeOAuth,
        },
    )
    .expect("store native Codex credential");

    let credential = resolve_provider_credential_with_config(
        CredentialRequest::new(
            "Codex",
            "TCUI_TODO8_CODEX_KEY",
            "https://chatgpt.com/backend-api/codex",
        ),
        &config,
    )
    .await
    .expect("resolve native Codex credential")
    .expect("native Codex credential");

    assert_eq!(credential.source(), CredentialSource::TcuiNativeOAuth);
}

#[tokio::test]
async fn resolver_never_uses_api_key_stores_for_gemini() {
    let _guard = crate::test_support::env_lock()
        .lock()
        .expect("environment lock");
    let environment = NativeTestEnvironment::new("resolver-gemini-type-boundary");
    let config = environment.config();
    KeyStore::save_keys(&config, &[("Gemini".to_string(), "legacy-key".to_string())])
        .expect("store legacy Gemini key");
    let stored = ApiKeyCredential::new(
        "Gemini",
        "stored-key",
        ApiKeyCredentialOwnership::Tcui,
        ApiKeyCredentialSource::OpenRouterPkce,
    )
    .expect("create stored Gemini key");
    KeyStore::upsert_credential(&config, &Credential::ApiKey(stored))
        .expect("store typed Gemini key");

    let credential = resolve_provider_credential_with_config(
        CredentialRequest::new(
            "Gemini",
            "TCUI_TODO8_GEMINI_KEY",
            "https://generativelanguage.googleapis.com/v1beta",
        ),
        &config,
    )
    .await
    .expect("resolve Gemini credential");

    assert!(credential.is_none());
}
