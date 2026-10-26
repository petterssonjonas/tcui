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
    let program = commands::authorize(config, &config.notifications.pushover.command)
        .map_err(|_| eyre!("Pushover helper is not available under the current command policy"))?;
    let (user_key, app_key) = credentials(
        config,
        |name| std::env::var(name).ok(),
        |id| {
            KeyStore::get_secret(config, id)
                .map_err(|_| eyre!("Pushover requires an unlocked credential vault"))
        },
    )?;
    Ok(Some(Delivery {
        program,
        user_key,
        app_key,
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
        if !self.program.is_absolute() {
            return Err(eyre!("Invalid Pushover helper snapshot"));
        }
        let program = self
            .program
            .to_str()
            .ok_or_else(|| eyre!("Invalid Pushover helper path"))?;
        let arguments = ["--app", "tcui", "--title", title, "--", body].map(str::to_owned);
        let output = commands::run(
            config,
            program,
            &arguments,
            &[
                ("PUSHOVER_USER_KEY", self.user_key.as_str()),
                ("PUSHOVER_TCUI_APP_TOKEN", self.app_key.as_str()),
                // Prevent the helper from opening its ambient default credential file.
                ("PUSHOVER_ENV_FILE", ""),
            ],
        )
        .await
        .map_err(|_| eyre!("Pushover helper could not complete delivery"))?;
        if !output.success {
            // A helper can echo credentials on either stream; never forward diagnostics.
            return Err(eyre!("Pushover helper rejected delivery"));
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

fn credentials(
    config: &AppConfig,
    get: impl Fn(&str) -> Option<String>,
    vault: impl Fn(&str) -> Result<Option<String>>,
) -> Result<(String, String)> {
    let settings = &config.notifications.pushover;
    let (mut user, mut app) = if settings.use_vault {
        // This branch must never inspect environment or file credentials, even on failure.
        let user = Zeroizing::new(
            vault(USER_KEY_ID)?
                .ok_or_else(|| eyre!("Pushover user key is missing from the vault"))?,
        );
        let app = Zeroizing::new(
            vault(APP_KEY_ID)?
                .ok_or_else(|| eyre!("Pushover application key is missing from the vault"))?,
        );
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
        let app = values
            .remove("PUSHOVER_TCUI_APP_TOKEN")
            .or_else(|| values.remove("PUSHOVER_APP_TOKEN"))
            .ok_or_else(|| {
                eyre!("Pushover application key is missing from the configured source")
            })?;
        (user, app)
    };
    validate_key(&user)?;
    validate_key(&app)?;
    Ok((std::mem::take(&mut *user), std::mem::take(&mut *app)))
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
            |_| panic!("vault mode read environment"),
            |id| { Ok((id == USER_KEY_ID).then(|| USER.to_string())) }
        )
        .is_err());
    }

    #[test]
    fn tcui_token_precedes_generic_token_and_validation_redacts_values() {
        let config = AppConfig::default();
        let (user, app) = credentials(
            &config,
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
            |name| (name == "PUSHOVER_APP_TOKEN").then(|| OTHER.into()),
            |_| panic!("vault read"),
        )
        .expect("file with environment override");
        assert_eq!(app, OTHER);
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
    #[tokio::test]
    async fn helper_failure_redacts_output_and_current_policy_can_revoke_snapshot() {
        use std::os::unix::fs::PermissionsExt;
        let root = std::env::temp_dir().join(format!(
            "tcui-pushover-send-{}-{:032x}",
            std::process::id(),
            rand::random::<u128>()
        ));
        std::fs::create_dir(&root).expect("fixture directory");
        let program = root.join("notify-ping");
        // Echo both trusted secrets to exercise the diagnostic suppression boundary.
        std::fs::write(&program, "#!/bin/sh\nprintf 'ran' > \"$0.sentinel\"\nprintf '%s' \"$PUSHOVER_USER_KEY\"\nprintf '%s' \"$PUSHOVER_TCUI_APP_TOKEN\" >&2\nexit 1\n").expect("fake helper");
        std::fs::set_permissions(&program, std::fs::Permissions::from_mode(0o700))
            .expect("executable");
        let delivery = Delivery {
            program: program.clone(),
            user_key: USER.into(),
            app_key: APP.into(),
        };
        let mut config = AppConfig::default();
        config.notifications.pushover.enabled = true;
        config.commands.whitelist = vec![program.to_string_lossy().into_owned()];
        let error = delivery
            .send(&config, "title", "body")
            .await
            .expect_err("helper failure");
        let diagnostic = format!("{error:?}");
        assert!(!diagnostic.contains(USER));
        assert!(!diagnostic.contains(APP));
        let sentinel = program.with_extension("sentinel");
        assert_eq!(
            std::fs::read(&sentinel).expect("helper execution evidence"),
            b"ran"
        );
        std::fs::remove_file(&sentinel).expect("reset execution evidence");
        config.commands.blacklist = config.commands.whitelist.clone();
        assert!(delivery.send(&config, "title", "body").await.is_err());
        assert!(!sentinel.exists(), "blacklisted helper executed");
        std::fs::remove_dir_all(root).expect("remove fixture");
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
