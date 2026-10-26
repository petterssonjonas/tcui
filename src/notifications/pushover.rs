use std::{
    collections::HashMap,
    fs::OpenOptions,
    io::Read,
    path::{Path, PathBuf},
};

use color_eyre::{eyre::eyre, Result};
use serde::{Deserialize, Serialize};
use zeroize::{Zeroize, Zeroizing};

use crate::{
    commands,
    config::{AppConfig, KeyStore},
};

pub(crate) const USER_KEY_ID: &str = "tcui.pushover.user-key";
pub(crate) const APP_KEY_ID: &str = "tcui.pushover.app-key";
const MAX_ENV_BYTES: u64 = 65_536;
const CREDENTIAL_NAMES: [&str; 3] = [
    "PUSHOVER_USER_KEY",
    "PUSHOVER_TCUI_APP_TOKEN",
    "PUSHOVER_APP_TOKEN",
];

/// Only explicitly requested Pushover keys may be exposed to the model provider.
pub(crate) struct AgentCredentials {
    pub(crate) user_key: Zeroizing<String>,
    pub(crate) app_key: Zeroizing<String>,
    pub(crate) application: String,
}

pub(crate) fn credentials_for_agent(
    config: &AppConfig,
    application: Option<&str>,
) -> Result<AgentCredentials> {
    if !config.notifications.pushover.enabled {
        return Err(eyre!("Pushover is disabled"));
    }
    let application = application.unwrap_or(&config.notifications.pushover.application);
    let (user_key, app_key) = credentials(
        config,
        application,
        |name| std::env::var(name).ok(),
        |id| {
            KeyStore::get_pushover_secret(config, id)
                .map_err(|_| eyre!("Cannot read Pushover TCUI-keystore key; encrypted keys require an unlocked vault"))
        },
    )?;
    Ok(AgentCredentials {
        user_key,
        app_key,
        application: application.to_owned(),
    })
}

pub(crate) fn is_credential_id(id: &str) -> bool {
    id == USER_KEY_ID
        || id == APP_KEY_ID
        || id
            .strip_prefix("tcui.pushover.app-key:")
            .is_some_and(|name| name != "tcui" && application_key_id(name).is_ok())
}

/// Contains credentials: serialize only into the private, short-lived reminder job file.
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Delivery {
    program: PathBuf,
    user_key: String,
    app_key: String,
}

impl Drop for Delivery {
    fn drop(&mut self) {
        self.user_key.zeroize();
        self.app_key.zeroize();
    }
}

pub(crate) fn prepare(config: &AppConfig) -> Result<Option<Delivery>> {
    if !config.notifications.pushover.enabled {
        return Ok(None);
    }
    let program = commands::authorize(config, "curl")
        .map_err(|_| eyre!("Pushover curl is not available under the current command policy"))?;
    let mut credentials = credentials_for_agent(config, None)?;
    Ok(Some(Delivery {
        program,
        user_key: std::mem::take(&mut *credentials.user_key),
        app_key: std::mem::take(&mut *credentials.app_key),
    }))
}

impl Delivery {
    pub(crate) async fn send(&self, config: &AppConfig, title: &str, body: &str) -> Result<()> {
        if !config.notifications.pushover.enabled {
            return Err(eyre!("Pushover delivery is disabled"));
        }
        validate_key(&self.user_key)?;
        validate_key(&self.app_key)?;
        if title.chars().count() > 250
            || body.is_empty()
            || body.chars().count() > 1024
            || title.contains('\0')
            || body.contains('\0')
        {
            return Err(eyre!("Pushover title or message exceeds service limits"));
        }
        let current = commands::authorize(config, "curl").map_err(|_| {
            eyre!("Pushover curl is not available under the current command policy")
        })?;
        if !self.program.is_absolute() || self.program != current {
            return Err(eyre!("Invalid or obsolete Pushover transport snapshot"));
        }
        #[derive(Serialize)]
        struct Message<'a> {
            token: &'a str,
            user: &'a str,
            title: &'a str,
            message: &'a str,
        }
        let input = Zeroizing::new(
            serde_json::to_vec(&Message {
                token: &self.app_key,
                user: &self.user_key,
                title,
                message: body,
            })
            .map_err(|_| eyre!("Cannot encode Pushover delivery"))?,
        );
        let arguments = [
            "--disable",
            "--silent",
            "--fail",
            "--proto",
            "=https",
            "--connect-timeout",
            "10",
            "--max-time",
            "25",
            "--max-filesize",
            "65536",
            "--max-redirs",
            "0",
            "--request",
            "POST",
            "--header",
            "Content-Type: application/json",
            "--data-binary",
            "@-",
            "--url",
            "https://api.pushover.net/1/messages.json",
        ]
        .map(str::to_owned);
        let output = commands::run_with_input(config, "curl", &arguments, &input)
            .await
            .map_err(|_| eyre!("Pushover transport could not complete delivery"))?;
        if !output.success {
            return Err(eyre!("Pushover transport rejected delivery"));
        }
        #[derive(Deserialize)]
        struct Response {
            status: u8,
        }
        let response: Response = serde_json::from_str(&output.stdout)
            .map_err(|_| eyre!("Invalid Pushover API response"))?;
        if response.status != 1 {
            return Err(eyre!("Pushover API rejected delivery"));
        }
        Ok(())
    }
}

pub(crate) fn validate_key(value: &str) -> Result<()> {
    if value.len() != 30 || !value.bytes().all(|byte| byte.is_ascii_alphanumeric()) {
        return Err(eyre!(
            "Pushover keys must contain exactly 30 ASCII letters or digits"
        ));
    }
    Ok(())
}

pub(crate) fn application_key_id(application: &str) -> Result<String> {
    if application.is_empty()
        || application.len() > 64
        || !application.bytes().all(|byte| {
            byte.is_ascii_lowercase() || byte.is_ascii_digit() || matches!(byte, b'-' | b'_')
        })
    {
        return Err(eyre!("Pushover application names require 1-64 lowercase ASCII letters, digits, hyphens or underscores"));
    }
    Ok(if application == "tcui" {
        APP_KEY_ID.to_owned()
    } else {
        format!("{APP_KEY_ID}:{application}")
    })
}

fn credentials(
    config: &AppConfig,
    application: &str,
    get: impl Fn(&str) -> Option<String>,
    vault: impl Fn(&str) -> Result<Option<Zeroizing<String>>>,
) -> Result<(Zeroizing<String>, Zeroizing<String>)> {
    let settings = &config.notifications.pushover;
    let selected = application_key_id(application)?;
    let (user, app) = if settings.use_vault {
        // Never inspect environment or file credentials, including after a keystore failure.
        let user = vault(USER_KEY_ID)?
            .ok_or_else(|| eyre!("Pushover user key is missing from the TCUI keystore"))?;
        let app = vault(&selected)?
            .ok_or_else(|| eyre!("Pushover application key is missing from the TCUI keystore"))?;
        (user, app)
    } else {
        let mut values = match &settings.env_file {
            Some(path) => read_env_file(Path::new(path))?,
            None => HashMap::new(),
        };
        for name in CREDENTIAL_NAMES {
            if let Some(value) = get(name) {
                values.insert(name.to_string(), Zeroizing::new(value));
            }
        }
        let user = values
            .remove("PUSHOVER_USER_KEY")
            .ok_or_else(|| eyre!("Pushover user key is missing from the configured source"))?;
        let app = if application == "tcui" {
            values
                .remove("PUSHOVER_TCUI_APP_TOKEN")
                .or_else(|| values.remove("PUSHOVER_APP_TOKEN"))
        } else {
            values.remove("PUSHOVER_APP_TOKEN")
        }
        .ok_or_else(|| eyre!("Pushover application key is missing from the configured source"))?;
        (user, app)
    };
    validate_key(&user)?;
    validate_key(&app)?;
    Ok((user, app))
}

fn read_env_file(path: &Path) -> Result<HashMap<String, Zeroizing<String>>> {
    if !path.is_absolute() {
        return Err(eyre!("Pushover credential file must use an absolute path"));
    }
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK);
    }
    let file = options
        .open(path)
        .map_err(|_| eyre!("Cannot open Pushover credential file"))?;
    let metadata = file
        .metadata()
        .map_err(|_| eyre!("Cannot inspect Pushover credential file"))?;
    if !metadata.is_file() || metadata.len() > MAX_ENV_BYTES {
        return Err(eyre!(
            "Pushover credential file must be a regular file of at most 64 KiB"
        ));
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        if metadata.mode() & 0o777 != 0o600 || metadata.uid() != unsafe { libc::geteuid() } {
            return Err(eyre!(
                "Pushover credential file must be owned by this user with mode 0600"
            ));
        }
    }
    #[cfg(not(unix))]
    return Err(eyre!(
        "Private Pushover credential files require Unix permission checks"
    ));
    let mut text = Zeroizing::new(String::new());
    file.take(MAX_ENV_BYTES + 1)
        .read_to_string(&mut text)
        .map_err(|_| eyre!("Cannot read Pushover credential file"))?;
    if text.len() as u64 > MAX_ENV_BYTES {
        return Err(eyre!("Pushover credential file exceeds 64 KiB"));
    }
    parse_env(&text)
}

fn parse_env(text: &str) -> Result<HashMap<String, Zeroizing<String>>> {
    let mut values = HashMap::new();
    for raw in text.lines() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let line = line.strip_prefix("export ").unwrap_or(line).trim_start();
        let (name, raw_value) = line
            .split_once('=')
            .ok_or_else(|| eyre!("Invalid Pushover credential file assignment"))?;
        if name.is_empty()
            || !name.bytes().enumerate().all(|(index, byte)| {
                byte == b'_' || byte.is_ascii_alphabetic() || (index > 0 && byte.is_ascii_digit())
            })
        {
            return Err(eyre!("Invalid Pushover credential file assignment"));
        }
        if !CREDENTIAL_NAMES.contains(&name) {
            continue;
        }
        let value = raw_value.trim();
        let value = if value.starts_with(['\'', '"']) {
            let quote = value.as_bytes()[0] as char;
            value
                .strip_prefix(quote)
                .and_then(|value| value.strip_suffix(quote))
                .ok_or_else(|| eyre!("Invalid quoted Pushover credential file value"))?
        } else {
            value
        };
        if value.chars().any(|ch| {
            ch.is_control() || matches!(ch, '$' | '`' | ';' | '|' | '&' | '\\' | '\'' | '"')
        }) {
            return Err(eyre!(
                "Pushover credential files accept literal values only, not shell expressions"
            ));
        }
        if values
            .insert(name.to_string(), Zeroizing::new(value.to_string()))
            .is_some()
        {
            return Err(eyre!("Duplicate Pushover credential file assignment"));
        }
    }
    Ok(values)
}

#[cfg(test)]
mod tests {
    use super::*;

    const USER: &str = "uuuuuuuuuuuuuuuuuuuuuuuuuuuuuu";
    const APP: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
    const OTHER: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";

    #[test]
    fn vault_failure_never_reads_environment_or_file() {
        let mut config = AppConfig::default();
        config.notifications.pushover.use_vault = true;
        config.notifications.pushover.env_file = Some("/not/read".into());
        let result = credentials(
            &config,
            &config.notifications.pushover.application,
            |_| panic!("vault mode read environment"),
            |_| Err(eyre!("locked")),
        );
        assert!(result.is_err());
    }

    #[test]
    fn missing_vault_key_never_falls_back() {
        let mut config = AppConfig::default();
        config.notifications.pushover.use_vault = true;
        assert!(credentials(
            &config,
            &config.notifications.pushover.application,
            |_| panic!("vault mode read environment"),
            |id| { Ok((id == USER_KEY_ID).then(|| Zeroizing::new(USER.to_string()))) }
        )
        .is_err());
    }

    #[test]
    fn tcui_token_precedes_generic_token_and_validation_redacts_values() {
        let config = AppConfig::default();
        let (user, app) = credentials(
            &config,
            &config.notifications.pushover.application,
            |name| {
                Some(
                    match name {
                        "PUSHOVER_USER_KEY" => USER,
                        "PUSHOVER_TCUI_APP_TOKEN" => APP,
                        _ => OTHER,
                    }
                    .into(),
                )
            },
            |_| panic!("environment mode read vault"),
        )
        .expect("credentials");
        assert_eq!((user.as_str(), app.as_str()), (USER, APP));
        let error = validate_key("secret-canary-invalid").expect_err("invalid");
        assert!(!format!("{error:?}").contains("secret-canary"));
    }

    #[test]
    fn named_environment_application_never_uses_tcui_token() {
        let mut config = AppConfig::default();
        config.notifications.pushover.application = "work".into();
        let source = |name: &str| match name {
            "PUSHOVER_USER_KEY" => Some(USER.to_owned()),
            "PUSHOVER_TCUI_APP_TOKEN" => Some(APP.to_owned()),
            _ => None,
        };
        assert!(credentials(&config, "work", source, |_| panic!("vault read")).is_err());
        let (user, app) = credentials(
            &config,
            "work",
            |name| {
                if name == "PUSHOVER_APP_TOKEN" {
                    Some(OTHER.to_owned())
                } else {
                    source(name)
                }
            },
            |_| panic!("vault read"),
        )
        .expect("explicit selected application token");
        assert_eq!((user.as_str(), app.as_str()), (USER, OTHER));
    }

    #[test]
    fn env_parser_accepts_literals_without_evaluating_shell() {
        let values = parse_env(&format!("# comment\nOTHER_APP_TOKEN=\"${{OTHER_TOKEN}}; unrelated value\"\nOTHER_APP_COMMAND=$(id)\nexport PUSHOVER_USER_KEY='{USER}'\nPUSHOVER_APP_TOKEN=\"{APP}\"\n")).expect("mixed application env file");
        assert_eq!(values["PUSHOVER_USER_KEY"].as_str(), USER);
        assert_eq!(values["PUSHOVER_APP_TOKEN"].as_str(), APP);
        assert_eq!(values.len(), 2, "only Pushover credentials are imported");
        for text in [
            "PUSHOVER_USER_KEY=$(touch /tmp/no)",
            "PUSHOVER_USER_KEY=`id`",
            "PUSHOVER_USER_KEY='unterminated",
            "source other",
            "PUSHOVER_USER_KEY=a\nPUSHOVER_USER_KEY=b",
        ] {
            assert!(parse_env(text).is_err());
        }
    }

    #[cfg(unix)]
    #[test]
    fn env_file_enforces_permissions_bounds_and_environment_precedence() {
        use std::os::unix::fs::{symlink, PermissionsExt};
        let root = std::env::temp_dir().join(format!(
            "tcui-pushover-{}-{:032x}",
            std::process::id(),
            rand::random::<u128>()
        ));
        std::fs::create_dir(&root).expect("fixture directory");
        let path = root.join("credentials.env");
        std::fs::write(
            &path,
            format!("PUSHOVER_USER_KEY={USER}\nPUSHOVER_APP_TOKEN={APP}\n"),
        )
        .expect("fixture");
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644))
            .expect("permissions");
        assert!(read_env_file(&path).is_err());
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600))
            .expect("permissions");
        let mut config = AppConfig::default();
        config.notifications.pushover.env_file = Some(path.to_string_lossy().into_owned());
        let (_, app) = credentials(
            &config,
            &config.notifications.pushover.application,
            |name| (name == "PUSHOVER_APP_TOKEN").then(|| OTHER.into()),
            |_| panic!("vault read"),
        )
        .expect("file with environment override");
        assert_eq!(app.as_str(), OTHER);
        let link = root.join("link");
        symlink(&path, &link).expect("symlink");
        assert!(read_env_file(&link).is_err());
        std::fs::write(&path, vec![b'a'; MAX_ENV_BYTES as usize + 1]).expect("oversize");
        assert!(read_env_file(&path).is_err());
        std::fs::remove_dir_all(root).expect("remove fixture");
    }

    #[test]
    fn disabled_delivery_does_not_require_credentials_or_command() {
        assert!(prepare(&AppConfig::default()).expect("disabled").is_none());
    }

    #[cfg(unix)]
    #[test]
    fn curl_uses_private_json_checks_api_status_and_rechecks_policy() {
        use std::os::unix::fs::PermissionsExt;
        let _lock = crate::test_support::env_lock().lock().unwrap();
        let root = std::env::temp_dir().join(format!("tcui-curl-{:032x}", rand::random::<u128>()));
        std::fs::create_dir(&root).unwrap();
        let program = root.join("curl");
        struct RestorePath(Option<std::ffi::OsString>);
        impl Drop for RestorePath {
            fn drop(&mut self) {
                match &self.0 {
                    Some(path) => std::env::set_var("PATH", path),
                    None => std::env::remove_var("PATH"),
                }
            }
        }
        let _restore = RestorePath(std::env::var_os("PATH"));
        std::env::set_var("PATH", &root);
        let mut config = AppConfig::default();
        config.notifications.pushover.enabled = true;
        config.commands.whitelist = vec![program.to_string_lossy().into_owned()];
        let delivery = Delivery {
            program: program.clone(),
            user_key: USER.into(),
            app_key: APP.into(),
        };
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        for (response, accepted) in [
            (r#"{"status":1}"#, true),
            (r#"{"status":0,"errors":["private diagnostic"]}"#, false),
            ("not JSON", false),
            (r#"{"status":"1"}"#, false),
            (USER, false),
        ] {
            std::fs::write(&program, format!(
                "#!/bin/sh\n[ \"$1\" = --disable ] || exit 2\nprintf '%s\\n' \"$@\" > \"$0.argv\"\n/usr/bin/env > \"$0.env\"\n/bin/cat > \"$0.stdin\"\nprintf '%s' '{response}'\n"
            )).unwrap();
            std::fs::set_permissions(&program, std::fs::Permissions::from_mode(0o700)).unwrap();
            let result = runtime.block_on(delivery.send(&config, "literal title", "literal body"));
            assert_eq!(result.is_ok(), accepted);
            if let Err(error) = result {
                let diagnostic = format!("{error:?}");
                assert!(!diagnostic.contains(USER) && !diagnostic.contains(APP));
                assert!(!diagnostic.contains("private diagnostic"));
            }
            for extension in ["argv", "env"] {
                let text = std::fs::read_to_string(program.with_extension(extension)).unwrap();
                assert!(!text.contains(USER) && !text.contains(APP));
            }
            let args = std::fs::read_to_string(program.with_extension("argv")).unwrap();
            assert!(args.contains("https://api.pushover.net/1/messages.json"));
            assert!(!args.contains("literal title") && !args.contains("literal body"));
            let input: serde_json::Value =
                serde_json::from_slice(&std::fs::read(program.with_extension("stdin")).unwrap())
                    .unwrap();
            assert_eq!(
                input,
                serde_json::json!({"token":APP,"user":USER,"title":"literal title","message":"literal body"})
            );
        }
        std::fs::remove_file(program.with_extension("stdin")).unwrap();
        config.commands.blacklist = vec!["curl".into()];
        assert!(runtime
            .block_on(delivery.send(&config, "title", "body"))
            .is_err());
        assert!(!program.with_extension("stdin").exists());
        config.commands.blacklist.clear();
        let legacy = Delivery {
            program: root.join("obsolete-helper"),
            user_key: USER.into(),
            app_key: APP.into(),
        };
        assert!(runtime
            .block_on(legacy.send(&config, "title", "body"))
            .is_err());
        assert!(!program.with_extension("stdin").exists());
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn selected_vault_application_never_uses_another_token() {
        assert_eq!(application_key_id("tcui").unwrap(), APP_KEY_ID);
        assert_eq!(
            application_key_id("alerts_2").unwrap(),
            format!("{APP_KEY_ID}:alerts_2")
        );
        for invalid in ["", "UPPER", "../key", "with space", "é"] {
            assert!(application_key_id(invalid).is_err());
        }
        assert!(application_key_id(&"a".repeat(65)).is_err());
        let mut config = AppConfig::default();
        config.notifications.pushover.use_vault = true;
        config.notifications.pushover.application = "alerts".into();
        let selected = application_key_id("alerts").unwrap();
        let load = |id: &str| {
            Ok(match id {
                USER_KEY_ID => Some(USER.to_owned()),
                APP_KEY_ID => Some(OTHER.to_owned()),
                id if id == selected => Some(APP.to_owned()),
                _ => None,
            }
            .map(Zeroizing::new))
        };
        assert_eq!(
            credentials(&config, "alerts", |_| panic!("environment read"), load)
                .unwrap()
                .1
                .as_str(),
            APP
        );
        assert!(credentials(
            &config,
            "alerts",
            |_| panic!("environment read"),
            |id| {
                Ok(match id {
                    USER_KEY_ID => Some(USER.to_owned()),
                    APP_KEY_ID => Some(OTHER.to_owned()),
                    _ => None,
                }
                .map(Zeroizing::new))
            }
        )
        .is_err());
    }

    #[test]
    fn agent_keys_select_exact_profiles_and_keep_encrypted_failures_closed() {
        use crate::config::key_store::{test_support::TestEnv, CredentialStorage};
        let _lock = crate::test_support::env_lock().lock().unwrap();
        let env = TestEnv::new("pushover-agent-key-sources");
        let mut config = env.config();
        config.notifications.pushover.enabled = true;
        config.notifications.pushover.use_vault = true;
        config.notifications.pushover.env_file = Some("/must/not/read".into());
        KeyStore::initialize_vault(&config, "test-only-vault-password").unwrap();
        for (id, key, storage) in [
            (USER_KEY_ID.to_owned(), USER, CredentialStorage::Plaintext),
            (APP_KEY_ID.to_owned(), APP, CredentialStorage::Plaintext),
            (
                application_key_id("secure").unwrap(),
                OTHER,
                CredentialStorage::Encrypted,
            ),
        ] {
            KeyStore::set_pushover_secret(&config, &id, "Fixture", key, storage).unwrap();
        }
        KeyStore::set_secret(
            &config,
            "unrelated-machine",
            "Unrelated",
            crate::config::key_store::SecretKind::Password,
            "unrelated-canary",
        )
        .unwrap();
        KeyStore::lock_vault(&config).unwrap();
        let keys = credentials_for_agent(&config, None).unwrap();
        assert_eq!(keys.application, "tcui");
        assert_eq!((keys.user_key.as_str(), keys.app_key.as_str()), (USER, APP));
        assert!(credentials_for_agent(&config, Some("secure")).is_err());
        assert!(credentials_for_agent(&config, Some("missing")).is_err());
        assert!(credentials_for_agent(&config, Some("../unrelated-machine")).is_err());
        assert!(KeyStore::get_pushover_secret(&config, "unrelated-machine").is_err());
        assert!(KeyStore::get_secret(&config, "unrelated-machine").is_err());
        assert!(KeyStore::get_secret(&config, USER_KEY_ID).is_err());
        config.notifications.pushover.application = "secure".into();
        assert!(credentials_for_agent(&config, None).is_err());
        assert_eq!(
            credentials_for_agent(&config, Some("tcui"))
                .unwrap()
                .app_key
                .as_str(),
            APP
        );
        KeyStore::unlock_vault(&config, "test-only-vault-password").unwrap();
        let keys = credentials_for_agent(&config, None).unwrap();
        assert_eq!(keys.application, "secure");
        assert_eq!(keys.app_key.as_str(), OTHER);
        config.notifications.pushover.enabled = false;
        assert!(credentials_for_agent(&config, Some("tcui")).is_err());
    }

    #[test]
    fn agent_environment_override_uses_only_explicit_pushover_sources() {
        let _lock = crate::test_support::env_lock().lock().unwrap();
        let previous = CREDENTIAL_NAMES.map(|name| (name, std::env::var_os(name)));
        struct Restore([(&'static str, Option<std::ffi::OsString>); 3]);
        impl Drop for Restore {
            fn drop(&mut self) {
                for (name, value) in &self.0 {
                    if let Some(value) = value {
                        std::env::set_var(name, value);
                    } else {
                        std::env::remove_var(name);
                    }
                }
            }
        }
        let _restore = Restore(previous);
        std::env::set_var("PUSHOVER_USER_KEY", USER);
        std::env::set_var("PUSHOVER_TCUI_APP_TOKEN", APP);
        std::env::set_var("PUSHOVER_APP_TOKEN", OTHER);
        let mut config = AppConfig::default();
        config.notifications.pushover.enabled = true;
        let keys = credentials_for_agent(&config, None).unwrap();
        assert_eq!(keys.app_key.as_str(), APP);
        let keys = credentials_for_agent(&config, Some("work")).unwrap();
        assert_eq!(keys.application, "work");
        assert_eq!(
            (keys.user_key.as_str(), keys.app_key.as_str()),
            (USER, OTHER)
        );
        std::env::remove_var("PUSHOVER_APP_TOKEN");
        assert!(credentials_for_agent(&config, Some("work")).is_err());
        std::env::set_var("PUSHOVER_TCUI_APP_TOKEN", "invalid");
        std::env::set_var("PUSHOVER_APP_TOKEN", OTHER);
        assert!(
            credentials_for_agent(&config, None).is_err(),
            "invalid preferred token must not downgrade"
        );
    }

    #[tokio::test]
    async fn invalid_snapshot_and_service_limits_fail_before_execution() {
        let mut config = AppConfig::default();
        config.notifications.pushover.enabled = true;
        let delivery = Delivery {
            program: PathBuf::from("relative-helper"),
            user_key: USER.into(),
            app_key: APP.into(),
        };
        assert!(delivery.send(&config, "title", "body").await.is_err());
        assert!(delivery
            .send(&config, &"t".repeat(251), "body")
            .await
            .is_err());
        assert!(delivery
            .send(&config, "title", &"b".repeat(1025))
            .await
            .is_err());
        assert!(delivery.send(&config, "title", "").await.is_err());
    }
}
