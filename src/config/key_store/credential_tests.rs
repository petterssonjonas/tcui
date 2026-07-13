use super::test_support::{TestEnv, env_lock};
use super::*;

fn openrouter_credential(key: &str) -> Credential {
    Credential::ApiKey(
        ApiKeyCredential::new(
            "OpenRouter",
            key,
            ApiKeyCredentialOwnership::Tcui,
            ApiKeyCredentialSource::OpenRouterPkce,
        )
        .expect("valid OpenRouter credential fixture"),
    )
}

#[test]
fn typed_api_key_round_trip_preserves_legacy_keys_and_redacts_raw_store() {
    // Given
    let _guard = env_lock().lock().expect("env lock poisoned");
    let env = TestEnv::new("typed-api-key-round-trip");
    let config = env.config();
    KeyStore::save_keys(&config, &[("OpenAI".to_owned(), "legacy-key".to_owned())])
        .expect("persist legacy key");
    let credential = openrouter_credential("openrouter-key-canary");

    // When
    KeyStore::upsert_credential(&config, &credential).expect("persist typed credential");
    let stored = KeyStore::get_credential(&config, "OpenRouter").expect("load credential");
    let raw = std::fs::read_to_string(config.key_file.as_deref().expect("key file path"))
        .expect("read encrypted key file");

    // Then
    assert_eq!(stored, Some(credential));
    assert!(!raw.contains("openrouter-key-canary"));
    assert_eq!(
        KeyStore::get(&config, "OpenAI")
            .expect("read legacy key")
            .as_deref(),
        Some("legacy-key")
    );
}

#[test]
fn typed_api_key_replacement_is_atomic_and_local_removal_is_idempotent() {
    // Given
    let _guard = env_lock().lock().expect("env lock poisoned");
    let env = TestEnv::new("typed-api-key-replacement");
    let config = env.config();
    KeyStore::upsert_credential(&config, &openrouter_credential("first-key"))
        .expect("persist first credential");

    // When
    KeyStore::upsert_credential(&config, &openrouter_credential("replacement-key"))
        .expect("replace credential");
    let removed = KeyStore::remove_api_key(
        &config,
        "OpenRouter",
        ApiKeyCredentialSource::OpenRouterPkce,
    )
    .expect("remove owned key");
    let removed_again = KeyStore::remove_api_key(
        &config,
        "OpenRouter",
        ApiKeyCredentialSource::OpenRouterPkce,
    )
    .expect("repeat local removal");

    // Then
    assert_eq!(
        KeyStore::get_credential(&config, "OpenRouter").expect("read removed credential"),
        None
    );
    assert!(removed && !removed_again);
}

#[test]
fn typed_api_key_debug_does_not_expose_the_secret() {
    // Given / When
    let rendered = format!("{:?}", openrouter_credential("key-debug-canary"));

    // Then
    assert!(!rendered.contains("key-debug-canary"));
}
