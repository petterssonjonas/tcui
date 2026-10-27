use super::AppConfig;
use crate::config::app_config::default_artifact_save_dir;
use std::path::PathBuf;
use std::sync::Mutex;

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

#[test]
fn save_writes_to_xdg_config_dir() {
    let _guard = env_lock().lock().expect("env lock poisoned");
    let root = unique_temp_dir("config-save-repo");
    let config_home = root.join("config-home");
    let tcui_dir = config_home.join("tcui");
    std::fs::create_dir_all(&tcui_dir).expect("create xdg config dir");
    std::fs::create_dir_all(&root).expect("create repo dir");
    std::env::set_var("XDG_CONFIG_HOME", &config_home);

    let config = AppConfig {
        theme: "gruvbox".to_string(),
        default_model: "repo-model".to_string(),
        ..AppConfig::default()
    };
    config.save().expect("save config");

    let xdg_path = tcui_dir.join("config.toml");
    let xdg_content = std::fs::read_to_string(&xdg_path).expect("read xdg config");
    assert!(xdg_content.contains("theme = \"gruvbox\""));
    assert!(xdg_content.contains("default_model = \"repo-model\""));
    assert!(!root.join("config.toml").exists());

    std::fs::remove_dir_all(&root).expect("cleanup temp dir");
    std::env::remove_var("XDG_CONFIG_HOME");
}

#[test]
fn load_fills_missing_xdg_fields_without_clobbering_existing_values() {
    let _guard = env_lock().lock().expect("env lock poisoned");
    let root = unique_temp_dir("config-save-xdg");
    let config_home = root.join("config-home");
    let data_home = root.join("data-home");
    std::fs::create_dir_all(&root).expect("create repo dir");
    std::env::set_var("XDG_CONFIG_HOME", &config_home);
    std::env::set_var("XDG_DATA_HOME", &data_home);
    let original_dir = std::env::current_dir().expect("current dir");
    std::env::set_current_dir(&root).expect("set current dir");

    let xdg_path = config_home.join("tcui").join("config.toml");
    std::fs::create_dir_all(xdg_path.parent().expect("xdg parent")).expect("create xdg dir");
    std::fs::write(&xdg_path, "theme = \"solarized-dark\"\n").expect("seed xdg config");

    let config = AppConfig::load().expect("load config");

    assert_eq!(config.theme, "solarized-dark");
    assert_eq!(
        config.artifact_save_dir.as_deref(),
        Some(default_artifact_save_dir().as_str())
    );
    let xdg_content = std::fs::read_to_string(&xdg_path).expect("read xdg config");
    assert!(xdg_content.contains("theme = \"solarized-dark\""));
    assert!(xdg_content.contains("artifact_save_dir ="));
    assert!(!root.join("config.toml").exists());

    std::env::set_current_dir(original_dir).expect("restore current dir");
    std::fs::remove_dir_all(&root).expect("cleanup temp dir");
    std::env::remove_var("XDG_CONFIG_HOME");
    std::env::remove_var("XDG_DATA_HOME");
}

#[test]
fn load_bootstraps_xdg_layout_on_first_run() {
    let _guard = env_lock().lock().expect("env lock poisoned");
    let root = unique_temp_dir("config-bootstrap");
    let config_home = root.join("config-home");
    let data_home = root.join("data-home");
    std::fs::create_dir_all(&root).expect("create repo dir");
    std::env::set_var("XDG_CONFIG_HOME", &config_home);
    std::env::set_var("XDG_DATA_HOME", &data_home);
    let original_dir = std::env::current_dir().expect("current dir");
    std::env::set_current_dir(&root).expect("set current dir");

    let config = AppConfig::load().expect("load default config");

    assert_eq!(config.theme, "system");
    assert_eq!(
        config.artifact_save_dir.as_deref(),
        Some(default_artifact_save_dir().as_str())
    );
    assert!(config_home.join("tcui").join("config.toml").exists());
    assert!(config_home.join("tcui").join("skills").exists());
    assert!(config_home.join("tcui").join("souls").exists());
    assert!(config_home.join("tcui").join("themes").exists());
    assert!(config_home.join("tcui").join("mcp").exists());
    assert!(data_home.join("tcui").join("keys.toml").exists());
    assert!(!root.join("config.toml").exists());

    std::env::set_current_dir(original_dir).expect("restore current dir");
    std::fs::remove_dir_all(&root).expect("cleanup temp dir");
    std::env::remove_var("XDG_CONFIG_HOME");
    std::env::remove_var("XDG_DATA_HOME");
}
