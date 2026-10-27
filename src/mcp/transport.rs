use crate::{
    config::{AppConfig, KeyStore, McpServerConfig},
    mcp::{
        error::{McpError, McpResult},
        registry::McpProfile,
    },
};
use reqwest::{
    header::{HeaderMap, HeaderValue, AUTHORIZATION},
    redirect::Policy,
    Url,
};
use rmcp::transport::{
    streamable_http_client::StreamableHttpClientTransportConfig, StreamableHttpClientTransport,
    TokioChildProcess,
};
use std::{process::Stdio, time::Duration};
use tokio::process::Command;
use zeroize::Zeroizing;

const RUNTIME_ENV: &[&str] = &[
    "PATH",
    "HOME",
    "USER",
    "TMPDIR",
    "XDG_CACHE_HOME",
    "XDG_CONFIG_HOME",
    "XDG_DATA_HOME",
    "LANG",
    "LC_ALL",
    "LC_CTYPE",
    "SSL_CERT_FILE",
    "SSL_CERT_DIR",
    "NODE_EXTRA_CA_CERTS",
    "REQUESTS_CA_BUNDLE",
    "CURL_CA_BUNDLE",
    "HTTP_PROXY",
    "HTTPS_PROXY",
    "ALL_PROXY",
    "NO_PROXY",
    "http_proxy",
    "https_proxy",
    "all_proxy",
    "no_proxy",
];

pub(crate) fn resolve_secret(
    profile: Option<&McpProfile>,
    config: &AppConfig,
) -> McpResult<Option<Zeroizing<String>>> {
    let Some(profile) = profile else {
        return Ok(None);
    };
    let Some(key_store_name) = profile.key_store_name else {
        return Ok(None);
    };
    if config
        .providers
        .iter()
        .any(|provider| provider.name.trim().eq_ignore_ascii_case(key_store_name))
    {
        return Err(McpError::CredentialCollision { key_store_name });
    }
    let value = KeyStore::get(config, key_store_name)
        .map_err(|_| McpError::CredentialStore { key_store_name })?
        .filter(|value| !value.trim().is_empty())
        .ok_or(McpError::MissingApiKey {
            profile: profile.name,
            key_store_name,
        })?;
    Ok(Some(Zeroizing::new(value)))
}

pub(crate) fn spawn_stdio(
    server: &McpServerConfig,
    profile: Option<&McpProfile>,
    config: &AppConfig,
    secret: Option<&str>,
) -> McpResult<TokioChildProcess> {
    let command = build_stdio_command(server, profile, config, secret)?;
    TokioChildProcess::builder(command)
        .stderr(Stdio::null())
        .spawn()
        .map(|(transport, _)| transport)
        .map_err(|error| {
            if error.kind() == std::io::ErrorKind::NotFound {
                McpError::MissingCommand
            } else {
                McpError::SpawnCommand
            }
        })
}

fn build_stdio_command(
    server: &McpServerConfig,
    profile: Option<&McpProfile>,
    _config: &AppConfig,
    secret: Option<&str>,
) -> McpResult<Command> {
    let program = server
        .command
        .as_deref()
        .filter(|value| !value.trim().is_empty())
        .ok_or(McpError::Configuration {
            detail: "stdio requires an explicit command",
        })?;
    let mut command = Command::new(program);
    command.args(server.args.as_deref().unwrap_or_default());
    command.kill_on_drop(true);
    command.env_clear();
    for name in RUNTIME_ENV {
        if let Some(value) = std::env::var_os(name) {
            command.env(name, value);
        }
    }
    if let Some(profile) = profile {
        if let (Some(env_var), Some(secret)) = (profile.api_key_env, secret) {
            command.env(env_var, secret);
        }
    }
    Ok(command)
}

pub(crate) fn validate_http_endpoint(
    server: &McpServerConfig,
    profile: Option<&McpProfile>,
) -> McpResult<Url> {
    let url = server
        .url
        .as_deref()
        .and_then(|value| Url::parse(value).ok())
        .ok_or(McpError::Configuration {
            detail: "streamable_http requires an absolute endpoint URL",
        })?;
    if !url.username().is_empty() || url.password().is_some() || url.fragment().is_some() {
        return Err(McpError::Configuration {
            detail: "endpoint must not contain credentials or a fragment; use TCUI KeyStore",
        });
    }
    if url.query_pairs().any(|(key, _)| {
        let key = key.to_ascii_lowercase();
        key.contains("key")
            || key.contains("token")
            || key.contains("auth")
            || key.contains("secret")
    }) {
        return Err(McpError::Configuration {
            detail: "endpoint query credentials are not permitted; use TCUI KeyStore",
        });
    }
    if let Some(expected) = profile.and_then(McpProfile::hosted_endpoint) {
        let expected = Url::parse(expected).map_err(|_| McpError::Configuration {
            detail: "invalid hosted preset",
        })?;
        if url.scheme() != "https"
            || url.host_str() != expected.host_str()
            || url.port_or_known_default() != Some(443)
        {
            return Err(McpError::Configuration { detail: "stored service keys may only be sent to the provider's official HTTPS MCP origin" });
        }
    } else if url.scheme() != "https"
        && !(url.scheme() == "http"
            && matches!(url.host_str(), Some("localhost" | "127.0.0.1" | "[::1]")))
    {
        return Err(McpError::Configuration {
            detail: "use HTTPS, or HTTP on a loopback address",
        });
    }
    Ok(url)
}

pub(crate) fn http_transport(
    url: Url,
    profile: Option<&McpProfile>,
    secret: Option<&str>,
) -> McpResult<StreamableHttpClientTransport<reqwest::Client>> {
    let mut headers = HeaderMap::new();
    if let Some(secret) = secret {
        let is_exa = profile.is_some_and(|profile| profile.skill == "exa");
        let mut value = if is_exa {
            HeaderValue::from_str(secret)
        } else {
            HeaderValue::from_str(&format!("Bearer {secret}"))
        }
        .map_err(|_| McpError::Configuration {
            detail: "stored API key is not a valid HTTP header value",
        })?;
        value.set_sensitive(true);
        headers.insert(
            if is_exa {
                reqwest::header::HeaderName::from_static("x-api-key")
            } else {
                AUTHORIZATION
            },
            value,
        );
    }
    let client = reqwest::Client::builder()
        .redirect(Policy::none())
        .connect_timeout(Duration::from_secs(10))
        .timeout(Duration::from_secs(35))
        .default_headers(headers)
        .build()
        .map_err(|_| McpError::Configuration {
            detail: "could not initialize secure MCP HTTP client",
        })?;
    // Never replay a paid tool call automatically after a session expires.
    let transport_config = StreamableHttpClientTransportConfig::with_uri(url.as_str())
        .reinit_on_expired_session(false);
    Ok(StreamableHttpClientTransport::with_client(
        client,
        transport_config,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn configured_stdio_command_and_args_are_honored_without_ambient_keys() {
        let _guard = crate::test_support::env_lock().lock().expect("env lock");
        let old_key = std::env::var_os("EXA_API_KEY");
        std::env::set_var("EXA_API_KEY", "ambient-secret");
        let profile = crate::mcp::profile_by_skill("exa").unwrap();
        let server = McpServerConfig {
            command: Some("my-server".into()),
            args: Some(vec!["--explicit".into()]),
            transport: "stdio".into(),
            ..profile.config(true)
        };
        let command = build_stdio_command(
            &server,
            Some(profile),
            &AppConfig::default(),
            Some("stored-secret"),
        )
        .unwrap();
        let command = command.as_std();
        assert_eq!(command.get_program(), "my-server");
        assert_eq!(
            command.get_args().collect::<Vec<_>>(),
            vec![std::ffi::OsStr::new("--explicit")]
        );
        let key = command
            .get_envs()
            .find(|(key, _)| *key == "EXA_API_KEY")
            .and_then(|(_, value)| value);
        assert_eq!(key, Some(std::ffi::OsStr::new("stored-secret")));
        assert!(!command.get_envs().any(|(key, _)| key == "TAVILY_API_KEY"));
        match old_key {
            Some(value) => std::env::set_var("EXA_API_KEY", value),
            None => std::env::remove_var("EXA_API_KEY"),
        }
    }

    #[test]
    fn provider_credentials_are_bound_to_official_origins() {
        for provider in ["exa", "tavily", "firecrawl"] {
            let profile = crate::mcp::profile_by_skill(provider).unwrap();
            let mut server = profile.config(true);
            assert!(validate_http_endpoint(&server, Some(profile)).is_ok());
            for endpoint in [
                "https://attacker.invalid/mcp",
                "http://mcp.exa.ai/mcp",
                "https://user:secret@mcp.exa.ai/mcp",
                "https://mcp.exa.ai/mcp?api_key=secret",
            ] {
                server.url = Some(endpoint.into());
                let error = validate_http_endpoint(&server, Some(profile)).unwrap_err();
                assert!(!format!("{error:?} {error}").contains("secret"));
            }
        }
    }
}

#[cfg(test)]
mod credential_tests {
    use super::*;

    struct IsolatedStore {
        root: std::path::PathBuf,
        data_home: Option<std::ffi::OsString>,
        ambient_key: Option<std::ffi::OsString>,
    }

    impl IsolatedStore {
        fn new() -> Self {
            let root = std::env::temp_dir().join(format!(
                "tcui-mcp-keys-{}-{}",
                std::process::id(),
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_nanos()
            ));
            let data_home = std::env::var_os("XDG_DATA_HOME");
            let ambient_key = std::env::var_os("EXA_API_KEY");
            std::env::set_var("XDG_DATA_HOME", &root);
            std::env::set_var("EXA_API_KEY", "ambient-must-not-be-used");
            Self {
                root,
                data_home,
                ambient_key,
            }
        }
    }

    impl Drop for IsolatedStore {
        fn drop(&mut self) {
            let _ = KeyStore::lock_vault(&AppConfig::default());
            match self.data_home.take() {
                Some(value) => std::env::set_var("XDG_DATA_HOME", value),
                None => std::env::remove_var("XDG_DATA_HOME"),
            }
            match self.ambient_key.take() {
                Some(value) => std::env::set_var("EXA_API_KEY", value),
                None => std::env::remove_var("EXA_API_KEY"),
            }
            let _ = std::fs::remove_dir_all(&self.root);
        }
    }

    #[test]
    fn missing_stored_keys_do_not_fall_back_to_ambient_environment() {
        let _guard = crate::test_support::env_lock().lock().unwrap();
        let _store = IsolatedStore::new();
        let config = AppConfig::default();
        KeyStore::set_storage_preference(
            &config,
            crate::config::key_store::CredentialStorage::Plaintext,
        )
        .unwrap();
        let profile = crate::mcp::profile_by_skill("exa");
        assert!(matches!(
            resolve_secret(profile, &config),
            Err(McpError::MissingApiKey { .. })
        ));
        KeyStore::set(&config, "Exa Search", "stored-canary").unwrap();
        assert_eq!(
            resolve_secret(profile, &config)
                .unwrap()
                .as_deref()
                .map(String::as_str),
            Some("stored-canary")
        );
    }

    #[test]
    fn locked_encrypted_store_has_actionable_secret_safe_error() {
        let _guard = crate::test_support::env_lock().lock().unwrap();
        let _store = IsolatedStore::new();
        let config = AppConfig::default();
        KeyStore::initialize_vault(&config, "test-only-password").unwrap();
        KeyStore::set(&config, "Exa Search", "stored-canary").unwrap();
        KeyStore::lock_vault(&config).unwrap();
        let error = resolve_secret(crate::mcp::profile_by_skill("exa"), &config).unwrap_err();
        assert!(matches!(error, McpError::CredentialStore { .. }));
        assert!(error.to_string().contains("unlock"));
        assert!(!format!("{error:?} {error}").contains("stored-canary"));
        assert!(!format!("{error:?} {error}").contains("ambient-must-not-be-used"));
    }
}

#[cfg(test)]
mod collision_tests {
    use super::*;

    #[test]
    fn chat_provider_key_names_are_never_reused_as_search_credentials() {
        let mut config = AppConfig::default();
        config
            .providers
            .push(crate::config::app_config::ProviderConfig {
                name: "Exa Search".to_string(),
                endpoint: "https://chat.example".to_string(),
                env_var: "CUSTOM_CHAT_KEY".to_string(),
                backend_type: "openai".to_string(),
                auth_type: "api_key".to_string(),
            });
        assert!(matches!(
            resolve_secret(crate::mcp::profile_by_skill("exa"), &config),
            Err(McpError::CredentialCollision {
                key_store_name: "Exa Search"
            })
        ));
    }
}
