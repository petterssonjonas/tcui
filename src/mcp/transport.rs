use crate::{
    config::{AppConfig, KeyStore},
    mcp::{
        error::{McpError, McpResult},
        registry::McpProfile,
    },
};
use rmcp::transport::TokioChildProcess;
use std::process::Stdio;
use tokio::process::Command;

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

pub fn spawn_stdio(profile: &McpProfile, config: &AppConfig) -> McpResult<TokioChildProcess> {
    let command = build_stdio_command(profile, config)?;
    TokioChildProcess::builder(command)
        .stderr(Stdio::null())
        .spawn()
        .map(|(transport, _)| transport)
        .map_err(|error| {
            if error.kind() == std::io::ErrorKind::NotFound {
                McpError::MissingCommand {
                    program: profile.program,
                    hint: install_hint(profile.program),
                }
            } else {
                McpError::SpawnCommand {
                    program: profile.program,
                    source: error,
                }
            }
        })
}

fn build_stdio_command(profile: &McpProfile, config: &AppConfig) -> McpResult<Command> {
    let mut command = Command::new(profile.program);
    command.args(profile.args);
    command.env_clear();
    for name in RUNTIME_ENV {
        if let Some(value) = std::env::var_os(name) {
            command.env(name, value);
        }
    }

    if let Some(env_var) = profile.vault_env {
        let vault_path = std::env::var(env_var)
            .ok()
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty())
            .or_else(|| {
                config
                    .vault_path
                    .as_deref()
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
                    .map(ToString::to_string)
            })
            .ok_or(McpError::MissingVaultPath {
                profile: profile.name,
                env_var,
            })?;
        command.env(env_var, vault_path);
    }

    if let Some(env_var) = profile.api_key_env {
        let key_store_name = profile.key_store_name.ok_or(McpError::MissingApiKey {
            profile: profile.name,
            env_var,
            key_store_name: profile.name,
        })?;
        let secret = resolve_secret(env_var, Some(key_store_name), config)?.ok_or(
            McpError::MissingApiKey {
                profile: profile.name,
                env_var,
                key_store_name,
            },
        )?;
        command.env(env_var, secret);
    }

    Ok(command)
}

fn resolve_secret(
    env_var: &str,
    key_store_name: Option<&'static str>,
    config: &AppConfig,
) -> McpResult<Option<String>> {
    if let Some(value) = std::env::var(env_var)
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
    {
        return Ok(Some(value));
    }

    match key_store_name {
        Some(key_store_name) => Ok(KeyStore::get(config, key_store_name)
            .map_err(|error| McpError::Io(std::io::Error::other(error.to_string())))?
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty())),
        None => Ok(None),
    }
}

fn install_hint(program: &str) -> &'static str {
    match program {
        "npx" => "install Node.js so `npx` is available",
        "uvx" => "install `uv` so `uvx` is available",
        _ => "install the missing command and ensure it is on PATH",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn child_inherits_only_allowlisted_runtime_environment() {
        // Given
        let _guard = crate::test_support::env_lock()
            .lock()
            .expect("env lock poisoned");
        let old_path = std::env::var_os("PATH");
        let old_home = std::env::var_os("HOME");
        std::env::set_var("PATH", "/test/bin");
        std::env::set_var("HOME", "/test/home");
        std::env::set_var("UNRELATED_SECRET", "must-not-leak");
        std::env::set_var("EXA_API_KEY", "exa-secret");
        let profile = McpProfile {
            program: "/usr/bin/env",
            args: &[],
            ..*crate::mcp::profile_by_name("Exa").expect("Exa profile")
        };

        // When
        let mut command =
            build_stdio_command(&profile, &AppConfig::default()).expect("build command");
        let output = command.as_std_mut().output().expect("run env");
        let environment = String::from_utf8(output.stdout).expect("UTF-8 environment");

        // Then
        assert!(environment.lines().any(|line| line == "PATH=/test/bin"));
        assert!(environment.lines().any(|line| line == "HOME=/test/home"));
        assert!(
            environment
                .lines()
                .any(|line| line == "EXA_API_KEY=exa-secret")
        );
        assert!(!environment.contains("UNRELATED_SECRET"));

        std::env::remove_var("UNRELATED_SECRET");
        std::env::remove_var("EXA_API_KEY");
        match old_path {
            Some(value) => std::env::set_var("PATH", value),
            None => std::env::remove_var("PATH"),
        }
        match old_home {
            Some(value) => std::env::set_var("HOME", value),
            None => std::env::remove_var("HOME"),
        }
    }

    #[test]
    fn obsidian_environment_vault_overrides_settings() {
        // Given
        let _guard = crate::test_support::env_lock()
            .lock()
            .expect("env lock poisoned");
        let old_vault = std::env::var_os("OBSIDIAN_VAULT_PATH");
        std::env::set_var("OBSIDIAN_VAULT_PATH", "/env/vault");
        let config = AppConfig {
            vault_path: Some("/settings/vault".to_string()),
            ..AppConfig::default()
        };
        let profile = crate::mcp::profile_by_name("Obsidian").expect("Obsidian profile");

        // When
        let command = build_stdio_command(profile, &config).expect("build command");
        let vault = command
            .as_std()
            .get_envs()
            .find(|(key, _)| *key == "OBSIDIAN_VAULT_PATH")
            .and_then(|(_, value)| value);

        // Then
        assert_eq!(vault, Some(std::ffi::OsStr::new("/env/vault")));
        match old_vault {
            Some(value) => std::env::set_var("OBSIDIAN_VAULT_PATH", value),
            None => std::env::remove_var("OBSIDIAN_VAULT_PATH"),
        }
    }
}
