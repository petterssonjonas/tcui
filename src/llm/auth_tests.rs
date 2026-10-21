use super::*;

#[test]
fn redacts_embedded_key_value_and_jsonish_secrets() {
    let text = r#"api_key=sk-test url=https://x.test?a=1&access_token=ya29.secret {"authorization":"Bearer eyJabc"}"#;
    let redacted = redact_secrets(text);
    assert!(!redacted.contains("sk-test"));
    assert!(!redacted.contains("ya29.secret"));
    assert!(!redacted.contains("eyJabc"));
}

#[test]
fn oauth_names_remain_distinct_from_api_key_connections() {
    assert_eq!(canonical_provider_name("OpenAI OAuth"), "Codex");
    assert_eq!(canonical_provider_name("OpenRouter"), "OpenRouter");
    assert_eq!(
        canonical_provider_name("OpenRouter OAuth"),
        "OpenRouter OAuth"
    );
}

#[test]
fn configuration_bootstrap_preserves_explicitly_saved_credentials() {
    let _guard = crate::test_support::env_lock().lock().unwrap();
    let _environment = super::resolver_tests::NativeTestEnvironment::new("bootstrap-preserves-key");
    let config = crate::config::AppConfig::default();
    crate::config::KeyStore::set(&config, "OpenAI", "explicit-key").unwrap();
    config.save().unwrap();
    let loaded = crate::config::AppConfig::load().unwrap();
    assert_eq!(
        crate::config::KeyStore::get(&loaded, "OpenAI")
            .unwrap()
            .as_deref(),
        Some("explicit-key")
    );
}

#[test]
fn redacts_saved_keys_without_assuming_a_vendor_prefix() {
    let redacted = redact_known_secret(
        "The server rejected opaque-custom-credential.",
        Some("opaque-custom-credential"),
    );
    assert_eq!(redacted, "The server rejected [redacted].");
}
