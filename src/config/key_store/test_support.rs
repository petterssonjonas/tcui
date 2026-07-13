use super::AppConfig;
use std::ffi::OsString;
use std::path::PathBuf;
use std::sync::Mutex;

pub(super) fn env_lock() -> &'static Mutex<()> {
    crate::test_support::env_lock()
}

fn unique_temp_dir(label: &str) -> PathBuf {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("system clock before epoch")
        .as_nanos();
    std::env::temp_dir().join(format!("tcui-{label}-{}-{nanos}", std::process::id()))
}

pub(super) struct TestEnv {
    root: PathBuf,
    original_data_home: Option<OsString>,
}

impl TestEnv {
    pub(super) fn new(label: &str) -> Self {
        let root = unique_temp_dir(label);
        std::fs::create_dir_all(&root).expect("create isolated root");
        let original_data_home = std::env::var_os("XDG_DATA_HOME");
        std::env::set_var("XDG_DATA_HOME", root.join("data"));
        Self {
            root,
            original_data_home,
        }
    }

    pub(super) fn config(&self) -> AppConfig {
        AppConfig {
            key_file: Some(
                self.root
                    .join("config-home")
                    .join("tcui")
                    .join("keys.toml")
                    .display()
                    .to_string(),
            ),
            ..AppConfig::default()
        }
    }

    pub(super) fn data_home(&self) -> PathBuf {
        self.root.join("data")
    }
}

impl Drop for TestEnv {
    fn drop(&mut self) {
        match self.original_data_home.take() {
            Some(value) => std::env::set_var("XDG_DATA_HOME", value),
            None => std::env::remove_var("XDG_DATA_HOME"),
        }
        let _ = std::fs::remove_dir_all(&self.root);
    }
}
