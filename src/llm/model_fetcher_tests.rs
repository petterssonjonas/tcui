#![cfg(debug_assertions)]

use std::ffi::OsString;
use std::path::PathBuf;

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

use super::*;
use crate::config::key_store::{
    CredentialStorage, OAuthCredential, OAuthCredentialOwnership, OAuthCredentialSource,
};
use crate::config::{AppConfig, KeyStore};

const API_BASE: &str = "https://chatgpt.com/backend-api/codex";
const CATALOG: &str = r#"{
    "models": [
        {"slug":"a-secondary","visibility":"list","supported_in_api":true,"priority":20},
        {"slug":"hidden","visibility":"hide","supported_in_api":false,"priority":0},
        {"slug":"z-current","visibility":"list","supported_in_api":false,"priority":1,
         "context_window":272000,"default_reasoning_level":"high",
         "supported_reasoning_levels":[{"effort":"medium"},{"effort":"high"}]},
        {"slug":"another-current","visibility":"list","supported_in_api":false,"priority":30},
        {"slug":"fourth-current","visibility":"list","supported_in_api":true,"priority":40},
        {"slug":"fifth-current","visibility":"list","supported_in_api":false,"priority":50},
        {"slug":"z-current","visibility":"list","supported_in_api":false,"priority":60},
        {"slug":" ","visibility":"list","supported_in_api":true,"priority":0}
    ]
}"#;

struct TestEnvironment {
    root: PathBuf,
    previous: Vec<(&'static str, Option<OsString>)>,
}

impl TestEnvironment {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!(
            "tcui-live-models-{}-{}",
            std::process::id(),
            rand::random::<u64>()
        ));
        std::fs::create_dir_all(root.join("home/.codex")).expect("create fixture home");
        let previous = [
            "XDG_CONFIG_HOME",
            "XDG_DATA_HOME",
            "HOME",
            "TCUI_AUTH_TEST_CODEX_API_BASE",
        ]
        .into_iter()
        .map(|name| (name, std::env::var_os(name)))
        .collect();
        std::env::set_var("XDG_CONFIG_HOME", root.join("config"));
        std::env::set_var("XDG_DATA_HOME", root.join("data"));
        std::env::set_var("HOME", root.join("home"));
        std::env::remove_var("TCUI_AUTH_TEST_CODEX_API_BASE");
        Self { root, previous }
    }

    fn save_credential(&self) -> Result<(), Box<dyn std::error::Error>> {
        let config = AppConfig {
            providers: crate::config::app_config::provider_presets(),
            ..AppConfig::default()
        };
        config.save()?;
        KeyStore::set_storage_preference(&config, CredentialStorage::Plaintext)?;
        KeyStore::upsert_oauth(
            &config,
            &OAuthCredential {
                provider: "Codex".to_string(),
                access_token: "discovery-fixture-access".to_string(),
                refresh_token: Some("discovery-fixture-refresh".to_string()),
                expires_at: chrono::Utc::now() + chrono::Duration::hours(1),
                account_id: Some("discovery-fixture-account".to_string()),
                ownership: OAuthCredentialOwnership::Tcui,
                source: OAuthCredentialSource::NativeOAuth,
            },
        )?;
        Ok(())
    }
}

impl Drop for TestEnvironment {
    fn drop(&mut self) {
        for (name, value) in &self.previous {
            match value {
                Some(value) => std::env::set_var(name, value),
                None => std::env::remove_var(name),
            }
        }
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

async fn fixture(
    status: &'static str,
    body: &'static str,
) -> Result<tokio::task::JoinHandle<std::io::Result<String>>, Box<dyn std::error::Error>> {
    let listener = TcpListener::bind("127.0.0.1:0").await?;
    std::env::set_var(
        "TCUI_AUTH_TEST_CODEX_API_BASE",
        format!("http://{}", listener.local_addr()?),
    );
    Ok(tokio::spawn(async move {
        let (mut socket, _) = listener.accept().await?;
        let mut request = Vec::new();
        let mut buffer = [0_u8; 1024];
        while !request.windows(4).any(|bytes| bytes == b"\r\n\r\n") {
            let count = socket.read(&mut buffer).await?;
            if count == 0 || request.len() > 8192 {
                return Err(std::io::Error::other("incomplete fixture request"));
            }
            request.extend_from_slice(&buffer[..count]);
        }
        socket
            .write_all(
                format!(
                    "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                )
                .as_bytes(),
            )
            .await?;
        socket.shutdown().await?;
        Ok(String::from_utf8_lossy(&request).into_owned())
    }))
}

fn cached_model(id: &str) -> crate::storage::db::ModelRow {
    (id.to_string(), None, None, None, None, Vec::new())
}

#[tokio::test]
async fn authenticated_discovery_replaces_the_obsolete_catalog_without_external_codex_files(
) -> Result<(), Box<dyn std::error::Error>> {
    let _guard = crate::test_support::env_lock().lock().expect("env lock");
    let environment = TestEnvironment::new();
    environment.save_credential()?;
    std::fs::write(
        environment.root.join("home/.codex/version.json"),
        r#"{"latest_version":"0.0.1"}"#,
    )?;
    std::fs::write(
        environment.root.join("home/.codex/config.toml"),
        "model = 'external-poison-model'",
    )?;
    let storage = crate::storage::Storage::new()?;
    storage.save_models(
        "Codex",
        &[
            cached_model("retired-one"),
            cached_model("retired-two"),
            cached_model("retired-three"),
        ],
    )?;
    let server = fixture("200 OK", CATALOG).await?;

    let models = refresh_provider_models(&storage, "Codex", API_BASE, "CODEX_API_KEY", "codex")
        .await
        .map_err(std::io::Error::other)?;
    let request = server.await??.to_ascii_lowercase();
    let ids: Vec<_> = models.iter().map(|model| model.id.as_str()).collect();
    assert_eq!(
        ids,
        [
            "z-current",
            "a-secondary",
            "another-current",
            "fourth-current",
            "fifth-current"
        ]
    );
    assert_eq!(
        storage
            .get_models("Codex")?
            .iter()
            .map(|model| model.0.as_str())
            .collect::<Vec<_>>(),
        ids
    );
    assert_eq!(models[0].default_reasoning_effort.as_deref(), Some("high"));
    assert_eq!(models[0].supported_reasoning_efforts, ["medium", "high"]);
    assert!(request.starts_with("get /models?client_version=0.163.0 http/1.1"));
    assert!(request.contains("authorization: bearer discovery-fixture-access"));
    assert!(request.contains("chatgpt-account-id: discovery-fixture-account"));
    assert!(request.contains("originator: codex_cli_rs"));
    assert!(request.contains("user-agent: codex_cli_rs/0.163.0 "));
    assert!(request.contains("accept: application/json"));
    assert!(!request.contains("version:"));
    Ok(())
}

#[tokio::test]
async fn failed_discovery_preserves_the_last_catalog_without_synthesizing_models(
) -> Result<(), Box<dyn std::error::Error>> {
    let _guard = crate::test_support::env_lock().lock().expect("env lock");
    let environment = TestEnvironment::new();
    environment.save_credential()?;
    let storage = crate::storage::Storage::new()?;
    let cached = vec![cached_model("last-discovered-model")];
    storage.save_models("Codex", &cached)?;
    let server = fixture(
        "503 Service Unavailable",
        r#"{"error":"temporarily unavailable"}"#,
    )
    .await?;

    let result =
        refresh_provider_models(&storage, "Codex", API_BASE, "CODEX_API_KEY", "codex").await;
    let _ = server.await??;

    assert!(result.is_err());
    assert_eq!(storage.get_models("Codex")?, cached);
    Ok(())
}

#[cfg(debug_assertions)]
#[test]
fn fixture_api_override_rejects_off_host_or_credential_bearing_urls() {
    let _guard = crate::test_support::env_lock().lock().expect("env lock");
    let _environment = TestEnvironment::new();
    for value in [
        "https://127.0.0.1:8080",
        "http://example.test:8080",
        "http://127.0.0.1",
        "http://user:password@127.0.0.1:8080",
        "http://127.0.0.1:8080?token=value",
        "http://127.0.0.1:8080#fragment",
    ] {
        std::env::set_var("TCUI_AUTH_TEST_CODEX_API_BASE", value);
        assert!(crate::llm::codex_responses::codex_api_base().is_err());
    }
}

#[tokio::test]
async fn startup_refresh_reloads_codex_even_with_a_fresh_cache_and_emits_the_new_catalog(
) -> Result<(), Box<dyn std::error::Error>> {
    let _guard = crate::test_support::env_lock().lock().expect("env lock");
    let environment = TestEnvironment::new();
    environment.save_credential()?;
    let storage = crate::storage::Storage::new()?;
    storage.save_models("Codex", &[cached_model("cached-before-login")])?;
    let server = fixture("200 OK", CATALOG).await?;
    let mut refreshed = Vec::new();

    refresh_all_models(&storage, |provider, models| {
        refreshed.push((provider, models))
    })
    .await;
    let _ = server.await??;

    assert_eq!(refreshed.len(), 1);
    assert_eq!(refreshed[0].0, "Codex");
    assert_eq!(refreshed[0].1[0].id, "z-current");
    Ok(())
}
