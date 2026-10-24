#![cfg(test)]

mod failure_cases;
mod migration_cases;

use std::path::PathBuf;
use std::sync::Mutex;

use super::persistence::{inject_failure, ConfigError, TestFailurePoint};
use super::AppConfig;

fn env_lock() -> &'static Mutex<()> {
    crate::test_support::env_lock()
}

fn unique_temp_dir(label: &str) -> PathBuf {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("system clock before epoch")
        .as_nanos();
    std::env::temp_dir().join(format!("tcui-{label}-{}-{nanos}", std::process::id()))
}

struct ConfigHome {
    root: PathBuf,
    previous_config_home: Option<std::ffi::OsString>,
    previous_data_home: Option<std::ffi::OsString>,
}

impl ConfigHome {
    fn new(label: &str) -> Self {
        let root = unique_temp_dir(label);
        let config_home = root.join("config-home");
        let data_home = root.join("data-home");
        std::fs::create_dir_all(config_home.join("tcui")).expect("create config home");
        let previous_config_home = std::env::var_os("XDG_CONFIG_HOME");
        let previous_data_home = std::env::var_os("XDG_DATA_HOME");
        std::env::set_var("XDG_CONFIG_HOME", &config_home);
        std::env::set_var("XDG_DATA_HOME", &data_home);
        Self {
            root,
            previous_config_home,
            previous_data_home,
        }
    }

    fn tcui_dir(&self) -> PathBuf {
        self.root.join("config-home/tcui")
    }

    fn config_path(&self) -> PathBuf {
        self.tcui_dir().join("config.toml")
    }

    fn json_path(&self) -> PathBuf {
        self.tcui_dir().join("config.json")
    }

    fn backup_path(&self) -> PathBuf {
        self.tcui_dir().join("config.toml.bak")
    }

    fn seed_toml(&self, content: &str) {
        std::fs::write(self.config_path(), content).expect("seed TOML config");
    }

    fn seed_json(&self, content: &str) {
        std::fs::write(self.json_path(), content).expect("seed JSON config");
    }
}

impl Drop for ConfigHome {
    fn drop(&mut self) {
        match self.previous_config_home.take() {
            Some(value) => std::env::set_var("XDG_CONFIG_HOME", value),
            None => std::env::remove_var("XDG_CONFIG_HOME"),
        }
        match self.previous_data_home.take() {
            Some(value) => std::env::set_var("XDG_DATA_HOME", value),
            None => std::env::remove_var("XDG_DATA_HOME"),
        }
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

fn serialized_config(config: &AppConfig) -> String {
    toml::to_string_pretty(config).expect("serialize test config")
}

fn assert_injected_failure(error: &color_eyre::Report, operation: &'static str) {
    assert!(matches!(
        error.downcast_ref::<ConfigError>(),
        Some(ConfigError::InjectedFailure {
            operation: actual
        }) if *actual == operation
    ));
}
