use super::*;
use crate::config::{AppConfig, KeyStore};
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::sync::MutexGuard;

fn lock_env() -> MutexGuard<'static, ()> {
    crate::test_support::env_lock()
        .lock()
        .unwrap_or_else(|poison| poison.into_inner())
}

fn unique_temp_dir(label: &str) -> PathBuf {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("system clock before epoch")
        .as_nanos();
    std::env::temp_dir().join(format!("tcui-{label}-{}-{nanos}", std::process::id()))
}

struct TestEnv {
    root: PathBuf,
    home: PathBuf,
    original_home: Option<OsString>,
    original_config_home: Option<OsString>,
    original_data_home: Option<OsString>,
}

impl TestEnv {
    fn new(label: &str) -> Self {
        let root = unique_temp_dir(label);
        let home = root.join("home");
        let config_home = root.join("config");
        let data_home = root.join("data");
        std::fs::create_dir_all(&home).expect("create isolated home");
        std::fs::create_dir_all(&config_home).expect("create isolated config home");
        std::fs::create_dir_all(&data_home).expect("create isolated data home");
        let original_home = std::env::var_os("HOME");
        let original_config_home = std::env::var_os("XDG_CONFIG_HOME");
        let original_data_home = std::env::var_os("XDG_DATA_HOME");
        std::env::set_var("HOME", &home);
        std::env::set_var("XDG_CONFIG_HOME", &config_home);
        std::env::set_var("XDG_DATA_HOME", &data_home);
        Self {
            root,
            home,
            original_home,
            original_config_home,
            original_data_home,
        }
    }

    fn home(&self) -> &Path {
        &self.home
    }
}

impl Drop for TestEnv {
    fn drop(&mut self) {
        restore_env("HOME", self.original_home.take());
        restore_env("XDG_CONFIG_HOME", self.original_config_home.take());
        restore_env("XDG_DATA_HOME", self.original_data_home.take());
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

struct EnvVarGuard {
    name: &'static str,
    original: Option<OsString>,
}

impl EnvVarGuard {
    fn replace(name: &'static str, value: Option<&str>) -> Self {
        let original = std::env::var_os(name);
        match value {
            Some(value) => std::env::set_var(name, value),
            None => std::env::remove_var(name),
        }
        Self { name, original }
    }
}

impl Drop for EnvVarGuard {
    fn drop(&mut self) {
        restore_env(self.name, self.original.take());
    }
}

fn restore_env(name: &str, value: Option<OsString>) {
    match value {
        Some(value) => std::env::set_var(name, value),
        None => std::env::remove_var(name),
    }
}

fn storage_with_key(provider: &str, key: &str) -> Storage {
    let config = AppConfig::default();
    config.save().expect("save isolated config");
    let storage = Storage::new().expect("create isolated storage");
    KeyStore::save_keys(&config, &[(provider.to_string(), key.to_string())])
        .expect("save encrypted API key");
    storage
}

#[test]
fn redacts_embedded_key_value_and_jsonish_secrets() {
    let text = r#"api_key=sk-test url=https://x.test?a=1&access_token=ya29.secret {"authorization":"Bearer eyJabc"}"#;
    let redacted = redact_secrets(text);
    assert!(!redacted.contains("sk-test"));
    assert!(!redacted.contains("ya29.secret"));
    assert!(!redacted.contains("eyJabc"));
}

#[test]
fn reads_gemini_token_from_dot_gemini_directory() {
    let _guard = lock_env();
    let env = TestEnv::new("gemini-oauth");
    let gemini_dir = env.home().join(".gemini");
    std::fs::create_dir_all(&gemini_dir).expect("create gemini dir");
    std::fs::write(
        gemini_dir.join("oauth_creds.json"),
        r#"{"access_token":"test-token"}"#,
    )
    .expect("write oauth creds");

    let token = read_oauth_token("Gemini");

    assert_eq!(token.as_deref(), Some("test-token"));
}

#[test]
fn reads_gemini_antigravity_session_token() {
    let _guard = lock_env();
    let env = TestEnv::new("gemini-antigravity-oauth");
    let session_dir = env.home().join(".gemini").join("antigravity");
    std::fs::create_dir_all(&session_dir).expect("create antigravity dir");
    std::fs::write(
        session_dir.join("session.json"),
        r#"{"token":{"access_token":"antigravity-token"}}"#,
    )
    .expect("write antigravity session");

    let token = read_oauth_token("Gemini");

    assert_eq!(token.as_deref(), Some("antigravity-token"));
}

#[test]
fn reads_codex_account_id_from_auth_file() {
    let _guard = lock_env();
    let env = TestEnv::new("codex-oauth");
    let codex_dir = env.home().join(".codex");
    std::fs::create_dir_all(&codex_dir).expect("create codex dir");
    std::fs::write(
        codex_dir.join("auth.json"),
        r#"{"tokens":{"access_token":"test-token","account_id":"account-123"}}"#,
    )
    .expect("write oauth credentials");

    let account_id = read_codex_account_id();

    assert_eq!(account_id.as_deref(), Some("account-123"));
}

#[test]
fn provider_api_key_prefers_environment_over_dotenv_and_encrypted_store() {
    let _guard = lock_env();
    let env = TestEnv::new("environment-precedence");
    let _api_key = EnvVarGuard::replace("OPENAI_API_KEY", Some("environment-key"));
    std::fs::write(env.home().join(".env"), "OPENAI_API_KEY=dotenv-key\n")
        .expect("write home dotenv");
    let storage = storage_with_key("OpenAI", "encrypted-key");

    let key = read_provider_api_key("OpenAI", "OPENAI_API_KEY", &storage);

    assert_eq!(key.as_deref(), Some("environment-key"));
}

#[test]
fn provider_api_key_prefers_dotenv_over_encrypted_store() {
    let _guard = lock_env();
    let env = TestEnv::new("dotenv-precedence");
    let _api_key = EnvVarGuard::replace("OPENAI_API_KEY", None);
    std::fs::write(env.home().join(".env"), "OPENAI_API_KEY=dotenv-key\n")
        .expect("write home dotenv");
    let storage = storage_with_key("OpenAI", "encrypted-key");

    let key = read_provider_api_key("OpenAI", "OPENAI_API_KEY", &storage);

    assert_eq!(key.as_deref(), Some("dotenv-key"));
}

#[test]
fn encrypted_api_key_is_readable_from_key_store_before_config_bootstrap() {
    let _guard = lock_env();
    let _env = TestEnv::new("encrypted-key");
    let _api_key = EnvVarGuard::replace("OPENAI_API_KEY", None);
    let config = AppConfig::default();
    config.save().expect("save isolated config");
    let _storage = Storage::new().expect("create isolated storage");
    KeyStore::save_keys(
        &config,
        &[("OpenAI".to_string(), "encrypted-key".to_string())],
    )
    .expect("save encrypted API key");

    let key = KeyStore::get(&config, "OpenAI").expect("read encrypted API key");

    assert_eq!(key.as_deref(), Some("encrypted-key"));
}

#[test]
fn provider_api_key_returns_typed_absence_after_config_bootstrap_rewrites_encrypted_store() {
    let _guard = lock_env();
    let _env = TestEnv::new("encrypted-key-resolution");
    let _api_key = EnvVarGuard::replace("OPENAI_API_KEY", None);
    let storage = storage_with_key("OpenAI", "encrypted-key");

    let key = read_provider_api_key("OpenAI", "OPENAI_API_KEY", &storage);

    assert!(key.is_none());
}

#[test]
fn provider_api_key_reads_external_codex_token_when_no_api_key_exists() {
    let _guard = lock_env();
    let env = TestEnv::new("codex-external-token");
    let _api_key = EnvVarGuard::replace("CODEX_API_KEY", None);
    let codex_dir = env.home().join(".codex");
    std::fs::create_dir_all(&codex_dir).expect("create codex dir");
    std::fs::write(
        codex_dir.join("auth.json"),
        r#"{"tokens":{"access_token":"codex-external-token"}}"#,
    )
    .expect("write codex credentials");
    let storage = Storage::new().expect("create isolated storage");

    let key = read_provider_api_key("Codex", "CODEX_API_KEY", &storage);

    assert_eq!(key.as_deref(), Some("codex-external-token"));
}

#[test]
fn provider_api_key_reads_external_gemini_token_when_no_api_key_exists() {
    let _guard = lock_env();
    let env = TestEnv::new("gemini-external-token");
    let _api_key = EnvVarGuard::replace("GEMINI_API_KEY", None);
    let gemini_dir = env.home().join(".gemini");
    std::fs::create_dir_all(&gemini_dir).expect("create gemini dir");
    std::fs::write(
        gemini_dir.join("oauth_creds.json"),
        r#"{"access_token":"gemini-external-token"}"#,
    )
    .expect("write gemini credentials");
    let storage = Storage::new().expect("create isolated storage");

    let key = read_provider_api_key("Gemini", "GEMINI_API_KEY", &storage);

    assert_eq!(key.as_deref(), Some("gemini-external-token"));
}

#[test]
fn external_token_returns_typed_absence_for_malformed_json() {
    let _guard = lock_env();
    let env = TestEnv::new("malformed-external-json");
    let codex_dir = env.home().join(".codex");
    std::fs::create_dir_all(&codex_dir).expect("create codex dir");
    std::fs::write(codex_dir.join("auth.json"), "{ malformed")
        .expect("write malformed credentials");

    let token = read_oauth_token("Codex");

    assert!(token.is_none());
}

#[test]
fn provider_api_key_returns_typed_absence_for_untrusted_endpoint() {
    let _guard = lock_env();
    let env = TestEnv::new("untrusted-endpoint");
    let _api_key = EnvVarGuard::replace("CODEX_API_KEY", None);
    let codex_dir = env.home().join(".codex");
    std::fs::create_dir_all(&codex_dir).expect("create codex dir");
    std::fs::write(
        codex_dir.join("auth.json"),
        r#"{"tokens":{"access_token":"untrusted-fixture-token"}}"#,
    )
    .expect("write codex credentials");
    let storage = Storage::new().expect("create isolated storage");
    storage
        .update_provider("Codex", "https://untrusted.example/v1", "openai", "oauth")
        .expect("set untrusted endpoint");

    let key = read_provider_api_key("Codex", "CODEX_API_KEY", &storage);

    assert!(key.is_none());
}

#[test]
fn provider_api_key_returns_typed_absence_when_credentials_are_missing() {
    let _guard = lock_env();
    let _env = TestEnv::new("missing-credentials");
    let _api_key = EnvVarGuard::replace("OPENAI_API_KEY", None);
    let storage = Storage::new().expect("create isolated storage");

    let key = read_provider_api_key("OpenAI", "OPENAI_API_KEY", &storage);

    assert!(key.is_none());
}
