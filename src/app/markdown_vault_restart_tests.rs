use std::path::PathBuf;

use crate::config::vault_profiles::{VaultCatalog, VaultProfile};
use crate::config::AppConfig;
use crate::ui::app_tabs::{AppTabs, AppView};

#[test]
fn markdown_vault_restart_persists_profiles_and_default_but_not_workspace_tabs() {
    // Given
    let _guard = crate::test_support::env_lock()
        .lock()
        .expect("environment lock poisoned");
    let root = std::env::var_os("TCUI_MARKDOWN_VAULT_VERIFY_ROOT")
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir)
        .join(format!(
            "profile-restart-{}-{}",
            std::process::id(),
            rand::random::<u64>()
        ));
    let config_home = root.join("config");
    let data_home = root.join("data");
    let cache_home = root.join("cache");
    let home = root.join("home");
    let first_root = root.join("first-vault");
    let second_root = root.join("second-vault");
    for directory in [
        &config_home,
        &data_home,
        &cache_home,
        &home,
        &first_root,
        &second_root,
    ] {
        std::fs::create_dir_all(directory).expect("create isolated restart fixture");
    }
    let original_environment = [
        ("XDG_CONFIG_HOME", std::env::var_os("XDG_CONFIG_HOME")),
        ("XDG_DATA_HOME", std::env::var_os("XDG_DATA_HOME")),
        ("XDG_CACHE_HOME", std::env::var_os("XDG_CACHE_HOME")),
        ("HOME", std::env::var_os("HOME")),
    ];
    std::env::set_var("XDG_CONFIG_HOME", &config_home);
    std::env::set_var("XDG_DATA_HOME", &data_home);
    std::env::set_var("XDG_CACHE_HOME", &cache_home);
    std::env::set_var("HOME", &home);

    let first = VaultProfile::new("First", &first_root).expect("create first profile");
    let second = VaultProfile::new("Second", &second_root).expect("create second profile");
    let second_id = second.id.clone();
    let initial = AppConfig::load().expect("load normalized built-in profile");
    let mut catalog = VaultCatalog {
        profiles: initial.vaults,
        default_id: initial.default_vault_id,
    };
    catalog.add(first).expect("persist first profile");
    catalog.add(second).expect("persist second profile");
    catalog
        .set_default(&second_id)
        .expect("persist selected default");
    let config = AppConfig {
        vaults: catalog.profiles.clone(),
        default_vault_id: catalog.default_id.clone(),
        ..AppConfig::default()
    };
    config.save().expect("save Vault profile catalog");
    let mut tabs = AppTabs::default();
    let launcher = tabs.add_launcher();
    tabs.replace_launcher(
        launcher,
        AppView::MarkdownVault {
            profile_id: second_id.clone(),
        },
    )
    .expect("open ephemeral Markdown Vault workspace");
    assert_eq!(tabs.tabs().len(), 2);
    drop((tabs, config));

    // When
    let restarted_config = AppConfig::load().expect("reload persisted Vault profile catalog");
    let restarted_tabs = AppTabs::default();

    // Then
    assert_eq!(restarted_config.vaults, catalog.profiles);
    assert_eq!(restarted_config.default_vault_id, Some(second_id));
    assert_eq!(restarted_tabs.tabs().len(), 1);
    assert!(matches!(
        restarted_tabs.active_tab().map(|tab| tab.view()),
        Some(AppView::Chat)
    ));

    std::fs::remove_dir_all(&root).expect("remove isolated restart fixture");
    for (name, value) in original_environment {
        match value {
            Some(value) => std::env::set_var(name, value),
            None => std::env::remove_var(name),
        }
    }
}
