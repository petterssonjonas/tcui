use std::ffi::OsString;
use std::path::PathBuf;

use super::{read_codex_account_id, read_oauth_token};

struct IsolatedHome {
    root: PathBuf,
    original_home: Option<OsString>,
    original_data_home: Option<OsString>,
}

impl IsolatedHome {
    fn new() -> std::io::Result<Self> {
        let root = std::env::temp_dir().join(format!(
            "tcui-codex-baseline-{}-{}",
            std::process::id(),
            rand::random::<u64>()
        ));
        std::fs::create_dir_all(&root)?;
        let original_home = std::env::var_os("HOME");
        let original_data_home = std::env::var_os("XDG_DATA_HOME");
        std::env::set_var("HOME", &root);
        std::env::set_var("XDG_DATA_HOME", root.join("data"));
        Ok(Self {
            root,
            original_home,
            original_data_home,
        })
    }
}

impl Drop for IsolatedHome {
    fn drop(&mut self) {
        match self.original_home.take() {
            Some(value) => std::env::set_var("HOME", value),
            None => std::env::remove_var("HOME"),
        }
        match self.original_data_home.take() {
            Some(value) => std::env::set_var("XDG_DATA_HOME", value),
            None => std::env::remove_var("XDG_DATA_HOME"),
        }
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

#[test]
fn codex_external_auth_is_read_in_place_without_creating_tcui_storage() -> std::io::Result<()> {
    let _guard = crate::test_support::env_lock()
        .lock()
        .expect("environment lock poisoned");
    let home = IsolatedHome::new()?;
    let codex_dir = home.root.join(".codex");
    std::fs::create_dir_all(&codex_dir)?;
    let auth_path = codex_dir.join("auth.json");
    let original =
        r#"{"tokens":{"access_token":"external-access-token","account_id":"account-123"}}"#;
    std::fs::write(&auth_path, original)?;

    let access_token = read_oauth_token("codex");
    let account_id = read_codex_account_id();

    assert_eq!(access_token.as_deref(), Some("external-access-token"));
    assert_eq!(account_id.as_deref(), Some("account-123"));
    assert_eq!(std::fs::read_to_string(&auth_path)?, original);
    assert!(!home.root.join("data/tcui/keys.toml").exists());
    Ok(())
}
