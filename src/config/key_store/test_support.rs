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

pub(crate) struct TestEnv {
    root: PathBuf,
    previous: [(&'static str, Option<OsString>); 4],
}

impl TestEnv {
    pub(crate) fn new(label: &str) -> Self {
        let root = unique_temp_dir(label);
        std::fs::create_dir_all(&root).expect("create isolated root");
        let previous = ["HOME", "XDG_CONFIG_HOME", "XDG_DATA_HOME", "XDG_CACHE_HOME"]
            .map(|name| (name, std::env::var_os(name)));
        for (name, directory) in [
            ("HOME", "home"),
            ("XDG_CONFIG_HOME", "config-home"),
            ("XDG_DATA_HOME", "data"),
            ("XDG_CACHE_HOME", "cache"),
        ] {
            let path = root.join(directory);
            std::fs::create_dir_all(&path).expect("create isolated directory");
            std::env::set_var(name, path);
        }
        Self { root, previous }
    }

    pub(crate) fn config(&self) -> AppConfig {
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

    pub(super) fn initialized_config(&self) -> AppConfig {
        let config = self.config();
        super::KeyStore::initialize_vault(&config, "test-only-vault-password")
            .expect("initialize isolated vault");
        config
    }

    pub(super) fn database_path(&self) -> PathBuf {
        self.data_home().join("tcui").join("tcui.db")
    }

    pub(super) fn data_home(&self) -> PathBuf {
        self.root.join("data")
    }
}

impl Drop for TestEnv {
    fn drop(&mut self) {
        let _ = crate::protection::lock();
        for (name, previous) in &self.previous {
            match previous {
                Some(value) => std::env::set_var(name, value),
                None => std::env::remove_var(name),
            }
        }
        let _ = std::fs::remove_dir_all(&self.root);
    }
}
