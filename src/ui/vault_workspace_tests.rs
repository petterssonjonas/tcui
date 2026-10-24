use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use super::app_tabs::{AppTabs, AppView};
use super::modals::terminal_editor::terminal_editor_test_support::live_editor;
use super::vault_workspace::{
    VaultWorkspaceError, VaultWorkspacePtyPoll, VaultWorkspaceRegistry, VaultWorkspaceRemoval,
};

pub(super) struct NativeVaultFixture {
    pub(super) root: PathBuf,
    environment_root: PathBuf,
    previous: Vec<(&'static str, Option<OsString>)>,
}

impl NativeVaultFixture {
    // The caller holds test_support::env_lock for the complete fixture lifetime.
    pub(super) fn new(label: &str) -> Self {
        let environment_root = std::env::temp_dir().join(format!(
            "tcui-native-vault-{label}-{}-{}",
            std::process::id(),
            rand::random::<u64>()
        ));
        let mut previous = Vec::new();
        for (name, directory) in [
            ("HOME", "home"),
            ("XDG_CONFIG_HOME", "config"),
            ("XDG_DATA_HOME", "data"),
            ("XDG_CACHE_HOME", "cache"),
        ] {
            previous.push((name, std::env::var_os(name)));
            let path = environment_root.join(directory);
            std::fs::create_dir_all(&path).expect("create isolated native vault environment");
            std::env::set_var(name, path);
        }
        let root = crate::protection::internal_vault_root();
        std::fs::create_dir_all(&root).expect("create native vault");
        Self {
            root,
            environment_root,
            previous,
        }
    }
}

impl Drop for NativeVaultFixture {
    fn drop(&mut self) {
        let _ = crate::protection::lock();
        for (name, value) in self.previous.drain(..) {
            match value {
                Some(value) => std::env::set_var(name, value),
                None => std::env::remove_var(name),
            }
        }
        let _ = std::fs::remove_dir_all(&self.environment_root);
    }
}

#[test]
fn activating_vault_again_focuses_existing_state_and_keeps_editor() {
    let _guard = crate::test_support::env_lock()
        .lock()
        .expect("env lock poisoned");
    let fixture = NativeVaultFixture::new("single-workspace");
    std::fs::write(fixture.root.join("note.md"), "native note").expect("write note");
    let mut tabs = AppTabs::default();
    let first_tab = tabs.add_launcher();
    let mut workspaces = VaultWorkspaceRegistry::default();
    workspaces
        .activate(&mut tabs, first_tab)
        .expect("activate Vault");
    let (editor, probe) = live_editor();
    let state = workspaces.get_mut_for_test(first_tab).expect("workspace");
    state.tree.select_path(Path::new("note.md"));
    state.viewer.scroll = 7;
    state.set_editor_for_test(editor);
    let launcher = tabs.add_launcher();

    workspaces
        .activate(&mut tabs, launcher)
        .expect("focus Vault");

    assert_eq!(workspaces.len(), 1);
    assert_eq!(tabs.active_id(), first_tab);
    assert_eq!(
        tabs.tab(launcher).map(super::app_tabs::AppTab::view),
        Some(&AppView::Launcher)
    );
    let state = workspaces.get(first_tab).expect("existing workspace");
    assert_eq!(state.tree.selected_path(), Some(Path::new("note.md")));
    assert_eq!(state.viewer.scroll, 7);
    assert!(state.editor.is_some());
    assert_eq!((probe.kills(), probe.waits()), (0, 0));

    let _ = tabs.close(launcher);
    assert!(matches!(
        workspaces.activate(&mut tabs, launcher),
        Err(VaultWorkspaceError::Activation(_))
    ));
    assert_eq!(tabs.active_id(), first_tab);
    assert_eq!((probe.kills(), probe.waits()), (0, 0));
}

#[test]
fn invalid_activation_keeps_chat_and_native_content_unchanged() {
    let _guard = crate::test_support::env_lock()
        .lock()
        .expect("env lock poisoned");
    let fixture = NativeVaultFixture::new("invalid-activation");
    std::fs::write(fixture.root.join("note.md"), "native note").expect("write note");
    let mut tabs = AppTabs::default();
    let chat = tabs.active_id();
    let mut workspaces = VaultWorkspaceRegistry::default();

    let result = workspaces.activate(&mut tabs, chat);

    assert!(matches!(result, Err(VaultWorkspaceError::Activation(_))));
    assert_eq!(tabs.active_id(), chat);
    assert_eq!(
        tabs.active_tab().map(super::app_tabs::AppTab::view),
        Some(&AppView::Chat)
    );
    assert_eq!(workspaces.len(), 0);
    assert_eq!(
        std::fs::read_to_string(fixture.root.join("note.md")).expect("read note"),
        "native note"
    );
}

#[test]
fn stale_app_tab_actions_are_typed_no_ops() {
    let _guard = crate::test_support::env_lock()
        .lock()
        .expect("env lock poisoned");
    let _fixture = NativeVaultFixture::new("stale");
    let mut tabs = AppTabs::default();
    let tab_id = tabs.add_launcher();
    let mut workspaces = VaultWorkspaceRegistry::default();
    workspaces
        .activate(&mut tabs, tab_id)
        .expect("activate Vault");
    assert!(matches!(
        workspaces.close(&mut tabs, tab_id),
        VaultWorkspaceRemoval::Removed { .. }
    ));

    assert_eq!(
        workspaces.poll_editor(tab_id),
        VaultWorkspacePtyPoll::UnknownTab
    );
    assert_eq!(
        workspaces.close(&mut tabs, tab_id),
        VaultWorkspaceRemoval::UnknownTab
    );
}

#[test]
fn close_drops_workspace_and_live_pty_exactly_once() {
    let _guard = crate::test_support::env_lock()
        .lock()
        .expect("env lock poisoned");
    let _fixture = NativeVaultFixture::new("drop-once");
    let mut tabs = AppTabs::default();
    let tab_id = tabs.add_launcher();
    let mut workspaces = VaultWorkspaceRegistry::default();
    workspaces
        .activate(&mut tabs, tab_id)
        .expect("activate Vault");
    let workspace_drops = Arc::new(AtomicUsize::new(0));
    let (editor, probe) = live_editor();
    let state = workspaces.get_mut_for_test(tab_id).expect("workspace");
    state.set_drop_counter_for_test(Arc::clone(&workspace_drops));
    state.set_editor_for_test(editor);

    let first = workspaces.close(&mut tabs, tab_id);
    let second = workspaces.close(&mut tabs, tab_id);

    assert!(matches!(first, VaultWorkspaceRemoval::Removed { .. }));
    assert_eq!(second, VaultWorkspaceRemoval::UnknownTab);
    assert_eq!(workspace_drops.load(Ordering::SeqCst), 1);
    assert_eq!((probe.kills(), probe.waits()), (1, 1));
    assert_eq!((probe.child_drops(), probe.master_drops()), (1, 1));
}
