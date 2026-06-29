use super::AppConfig;
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
fn save_prefers_existing_repo_config_path() {
    let _guard = env_lock().lock().expect("env lock poisoned");
    let root = unique_temp_dir("config-save-repo");
    let config_home = root.join("config-home");
    let tcui_dir = config_home.join("tcui");
    std::fs::create_dir_all(&tcui_dir).expect("create xdg config dir");
    std::fs::create_dir_all(&root).expect("create repo dir");
    std::env::set_var("XDG_CONFIG_HOME", &config_home);
    let original_dir = std::env::current_dir().expect("current dir");
    std::env::set_current_dir(&root).expect("set current dir");

    let repo_path = root.join("config.toml");
    std::fs::write(&repo_path, "theme = \"solarized-dark\"\n").expect("seed repo config");

    let config = AppConfig {
        theme: "gruvbox".to_string(),
        default_model: "repo-model".to_string(),
        ..AppConfig::default()
    };
    config.save().expect("save config");

    let repo_content = std::fs::read_to_string(&repo_path).expect("read repo config");
    assert!(repo_content.contains("theme = \"gruvbox\""));
    assert!(repo_content.contains("default_model = \"repo-model\""));
    assert!(!tcui_dir.join("config.toml").exists());

    std::env::set_current_dir(original_dir).expect("restore current dir");
    std::fs::remove_dir_all(&root).expect("cleanup temp dir");
    std::env::remove_var("XDG_CONFIG_HOME");
}

#[test]
fn save_falls_back_to_xdg_when_repo_config_is_missing() {
    let _guard = env_lock().lock().expect("env lock poisoned");
    let root = unique_temp_dir("config-save-xdg");
    let config_home = root.join("config-home");
    std::fs::create_dir_all(&root).expect("create repo dir");
    std::env::set_var("XDG_CONFIG_HOME", &config_home);
    let original_dir = std::env::current_dir().expect("current dir");
    std::env::set_current_dir(&root).expect("set current dir");

    let config = AppConfig {
        theme: "gruvbox".to_string(),
        default_model: "xdg-model".to_string(),
        ..AppConfig::default()
    };
    config.save().expect("save config");

    let xdg_path = config_home.join("tcui").join("config.toml");
    let xdg_content = std::fs::read_to_string(&xdg_path).expect("read xdg config");
    assert!(xdg_content.contains("theme = \"gruvbox\""));
    assert!(xdg_content.contains("default_model = \"xdg-model\""));
    assert!(!root.join("config.toml").exists());

    std::env::set_current_dir(original_dir).expect("restore current dir");
    std::fs::remove_dir_all(&root).expect("cleanup temp dir");
    std::env::remove_var("XDG_CONFIG_HOME");
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
