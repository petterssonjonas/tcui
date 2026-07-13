use std::time::Duration;

use reqwest::Url;

use crate::config::key_store::{
    ApiKeyCredential, ApiKeyCredentialOwnership, ApiKeyCredentialSource, Credential,
};
use crate::config::{AppConfig, KeyStore};
use crate::llm::auth::{
    oauth::{OAuthError, RedirectUri, oauth_cancellation},
    openrouter::{
        OpenRouterAdapter, OpenRouterError, OpenRouterTestEndpoints, persist_exchanged_key,
    },
};

use super::contract::PastedInput;
use super::fixture::{delayed_fixture, fixture, response};
use super::lifecycle::TestEnv;

fn grant(
    adapter: &OpenRouterAdapter,
    code: &str,
) -> Result<crate::llm::auth::openrouter::OpenRouterCodeGrant, OpenRouterError> {
    let authorization =
        adapter.begin_headless(RedirectUri::parse("http://127.0.0.1:7777/callback")?)?;
    authorization.complete_headless(&mut PastedInput::code(code))
}

fn prior_credential() -> Credential {
    Credential::ApiKey(
        ApiKeyCredential::new(
            "OpenRouter",
            "prior-key-canary",
            ApiKeyCredentialOwnership::Tcui,
            ApiKeyCredentialSource::OpenRouterPkce,
        )
        .expect("valid prior credential"),
    )
}

#[test]
fn cancelled_after_http_before_persist_preserves_prior_key()
-> Result<(), Box<dyn std::error::Error>> {
    // Given
    let _guard = crate::test_support::env_lock()
        .lock()
        .expect("env lock poisoned");
    let environment = TestEnv::new("openrouter-cancel-before-persist");
    let config = AppConfig::default();
    KeyStore::upsert_credential(&config, &prior_credential())?;
    let prior_bytes = std::fs::read(environment.key_file())?;
    let exchanged_key = "exchanged-key-canary".to_owned();
    let (cancellation, handle) = oauth_cancellation();

    // When
    handle.cancel();
    let error = persist_exchanged_key(&config, exchanged_key, &cancellation)
        .expect_err("cancelled persistence must not upsert a credential");
    let retained = KeyStore::get_credential(&config, "OpenRouter")?;
    let retained_bytes = std::fs::read(environment.key_file())?;

    // Then
    assert!(matches!(
        error,
        OpenRouterError::OAuth(OAuthError::Cancelled)
    ));
    assert_eq!(retained, Some(prior_credential()));
    assert_eq!(retained_bytes, prior_bytes);
    Ok(())
}

#[tokio::test]
#[expect(
    clippy::await_holding_lock,
    reason = "The process-wide environment fixture must remain isolated through async cleanup."
)]
async fn rejected_exchange_preserves_prior_key_and_redacts_code()
-> Result<(), Box<dyn std::error::Error>> {
    // Given
    let _guard = crate::test_support::env_lock()
        .lock()
        .expect("env lock poisoned");
    let _env = TestEnv::new("openrouter-rejected-exchange");
    let fixture = fixture(
        vec![response(
            "403 Forbidden",
            r#"{"error":{"message":"rejected-code-canary"}}"#,
        )],
        Duration::from_secs(1),
    )
    .await?;
    let config = AppConfig::default();
    KeyStore::upsert_credential(&config, &prior_credential())?;
    let prior_bytes = std::fs::read(_env.key_file())?;
    let (cancellation, _) = oauth_cancellation();

    // When
    let error = fixture
        .adapter
        .exchange_and_persist(
            &config,
            grant(&fixture.adapter, "rejected-code-canary")?,
            &cancellation,
        )
        .await
        .expect_err("rejected authorization code");
    let retained = KeyStore::get_credential(&config, "OpenRouter")?;
    let retained_bytes = std::fs::read(_env.key_file())?;
    let rendered = format!("{error:?} {error}");
    let _ = fixture.finish().await?;

    // Then
    assert!(matches!(
        error,
        OpenRouterError::OAuth(OAuthError::UnexpectedTokenStatus)
    ));
    assert_eq!(retained, Some(prior_credential()));
    assert_eq!(retained_bytes, prior_bytes);
    assert!(!rendered.contains("rejected-code-canary") && !rendered.contains("prior-key-canary"));
    Ok(())
}

#[tokio::test]
#[expect(
    clippy::await_holding_lock,
    reason = "The process-wide environment fixture must remain isolated through async cleanup."
)]
async fn expired_exchange_code_returns_typed_rejection_without_persisting()
-> Result<(), Box<dyn std::error::Error>> {
    // Given
    let _guard = crate::test_support::env_lock()
        .lock()
        .expect("env lock poisoned");
    let _env = TestEnv::new("openrouter-expired-exchange");
    let fixture = fixture(
        vec![response(
            "400 Bad Request",
            r#"{"error":{"message":"expired"}}"#,
        )],
        Duration::from_secs(1),
    )
    .await?;
    let config = AppConfig::default();
    let (cancellation, _) = oauth_cancellation();

    // When
    let error = fixture
        .adapter
        .exchange_and_persist(
            &config,
            grant(&fixture.adapter, "expired-code")?,
            &cancellation,
        )
        .await
        .expect_err("expired authorization code");
    let stored = KeyStore::get_credential(&config, "OpenRouter")?;
    let _ = fixture.finish().await?;

    // Then
    assert!(matches!(
        error,
        OpenRouterError::OAuth(OAuthError::UnexpectedTokenStatus)
    ));
    assert_eq!(stored, None);
    Ok(())
}

#[tokio::test]
#[expect(
    clippy::await_holding_lock,
    reason = "The process-wide environment fixture must remain isolated through async cleanup."
)]
async fn malformed_exchange_response_preserves_prior_key() -> Result<(), Box<dyn std::error::Error>>
{
    // Given
    let _guard = crate::test_support::env_lock()
        .lock()
        .expect("env lock poisoned");
    let _env = TestEnv::new("openrouter-malformed-exchange");
    let fixture = fixture(
        vec![response("200 OK", r#"{"key":""}"#)],
        Duration::from_secs(1),
    )
    .await?;
    let config = AppConfig::default();
    KeyStore::upsert_credential(&config, &prior_credential())?;
    let (cancellation, _) = oauth_cancellation();

    // When
    let error = fixture
        .adapter
        .exchange_and_persist(
            &config,
            grant(&fixture.adapter, "malformed-code")?,
            &cancellation,
        )
        .await
        .expect_err("empty exchanged key");
    let retained = KeyStore::get_credential(&config, "OpenRouter")?;
    let _ = fixture.finish().await?;

    // Then
    assert!(matches!(error, OpenRouterError::MalformedResponse));
    assert_eq!(retained, Some(prior_credential()));
    Ok(())
}

#[tokio::test]
#[expect(
    clippy::await_holding_lock,
    reason = "The process-wide environment fixture must remain isolated through async cleanup."
)]
async fn slow_exchange_observes_shared_deadline() -> Result<(), Box<dyn std::error::Error>> {
    // Given
    let _guard = crate::test_support::env_lock()
        .lock()
        .expect("env lock poisoned");
    let _env = TestEnv::new("openrouter-slow-exchange");
    let (adapter, server) =
        delayed_fixture(Duration::from_millis(10), Duration::from_millis(50)).await?;
    let config = AppConfig::default();
    let (cancellation, _) = oauth_cancellation();

    // When
    let error = adapter
        .exchange_and_persist(&config, grant(&adapter, "slow-code")?, &cancellation)
        .await
        .expect_err("slow exchange must time out");
    server.await??;

    // Then
    assert!(matches!(
        error,
        OpenRouterError::OAuth(OAuthError::TokenDeadline)
    ));
    Ok(())
}

#[tokio::test]
#[expect(
    clippy::await_holding_lock,
    reason = "The process-wide environment fixture must remain isolated through async cleanup."
)]
async fn cancelled_exchange_observes_shared_cancellation() -> Result<(), Box<dyn std::error::Error>>
{
    // Given
    let _guard = crate::test_support::env_lock()
        .lock()
        .expect("env lock poisoned");
    let _env = TestEnv::new("openrouter-cancelled-exchange");
    let adapter = OpenRouterAdapter::for_test(
        OpenRouterTestEndpoints::new(
            Url::parse("https://authorization.example/auth")?,
            Url::parse("http://127.0.0.1:9/auth/keys/code")?,
            Url::parse("http://127.0.0.1:9/auth/keys")?,
        ),
        Duration::from_secs(1),
    )?;
    let config = AppConfig::default();
    KeyStore::upsert_credential(&config, &prior_credential())?;
    let prior_bytes = std::fs::read(_env.key_file())?;
    let (cancellation, handle) = oauth_cancellation();
    handle.cancel();

    // When
    let error = adapter
        .exchange_and_persist(&config, grant(&adapter, "cancelled-code")?, &cancellation)
        .await
        .expect_err("cancelled exchange");
    let retained = KeyStore::get_credential(&config, "OpenRouter")?;
    let retained_bytes = std::fs::read(_env.key_file())?;

    // Then
    assert!(matches!(
        error,
        OpenRouterError::OAuth(OAuthError::Cancelled)
    ));
    assert_eq!(retained, Some(prior_credential()));
    assert_eq!(retained_bytes, prior_bytes);
    Ok(())
}
