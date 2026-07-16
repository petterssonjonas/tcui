use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use super::app_tabs::AppTabs;
use super::markdown_vault_workspace::{
    MarkdownVaultWorkspaceError, MarkdownVaultWorkspacePtyPoll, MarkdownVaultWorkspaceRebindCancel,
    MarkdownVaultWorkspaceRebindRequest, MarkdownVaultWorkspaceRegistry,
    MarkdownVaultWorkspaceRemoval,
};
use super::modals::terminal_editor::terminal_editor_test_support::live_editor;
use crate::config::vault_profiles::{VaultCatalog, VaultProfile, VaultProfileId};
use crate::vault::VaultRelativePath;

struct CatalogFixture {
    root: PathBuf,
    catalog: VaultCatalog,
    first_id: VaultProfileId,
    second_id: VaultProfileId,
}

impl CatalogFixture {
    fn new(label: &str) -> Self {
        let root = std::env::temp_dir().join(format!(
            "tcui-markdown-vault-workspace-{label}-{}-{}",
            std::process::id(),
            rand::random::<u64>()
        ));
        let first_root = root.join("first");
        let second_root = root.join("second");
        std::fs::create_dir_all(&first_root).expect("create first vault");
        std::fs::create_dir_all(&second_root).expect("create second vault");
        std::fs::write(first_root.join("note.md"), "first").expect("write first note");
        std::fs::write(second_root.join("note.md"), "second").expect("write second note");
        let first = VaultProfile::new("First", first_root).expect("first profile");
        let second = VaultProfile::new("Second", second_root).expect("second profile");
        let first_id = first.id.clone();
        let second_id = second.id.clone();
        let mut catalog = VaultCatalog::default();
        catalog.add(first).expect("add first profile");
        catalog.add(second).expect("add second profile");
        Self {
            root,
            catalog,
            first_id,
            second_id,
        }
    }

    fn note() -> VaultRelativePath {
        VaultRelativePath::parse(Path::new("note.md")).expect("relative note")
    }
}

impl Drop for CatalogFixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

#[test]
fn independent_tabs_own_separate_profiles_and_filesystems() {
    let fixture = CatalogFixture::new("independent");
    let mut tabs = AppTabs::default();
    let first_tab = tabs.add_launcher();
    let second_tab = tabs.add_launcher();
    let mut workspaces = MarkdownVaultWorkspaceRegistry::default();

    workspaces
        .activate(&mut tabs, &fixture.catalog, first_tab, &fixture.first_id)
        .expect("activate first");
    workspaces
        .activate(&mut tabs, &fixture.catalog, second_tab, &fixture.second_id)
        .expect("activate second");

    assert_eq!(workspaces.len(), 2);
    assert_eq!(
        workspaces
            .get(first_tab)
            .expect("first workspace")
            .vault_fs()
            .read_text(&CatalogFixture::note())
            .expect("read first"),
        "first"
    );
    assert_eq!(
        workspaces
            .get(second_tab)
            .expect("second workspace")
            .vault_fs()
            .read_text(&CatalogFixture::note())
            .expect("read second"),
        "second"
    );
}

#[test]
fn duplicate_profile_binding_to_same_tab_is_rejected() {
    let fixture = CatalogFixture::new("duplicate");
    let mut tabs = AppTabs::default();
    let tab_id = tabs.add_launcher();
    let mut workspaces = MarkdownVaultWorkspaceRegistry::default();
    workspaces
        .activate(&mut tabs, &fixture.catalog, tab_id, &fixture.first_id)
        .expect("activate first");

    let result = workspaces.request_rebind(&mut tabs, &fixture.catalog, tab_id, &fixture.first_id);

    assert!(matches!(
        result,
        Err(MarkdownVaultWorkspaceError::ProfileAlreadyBoundToTab { tab_id: id, .. })
            if id == tab_id
    ));
}

#[test]
fn failed_rebind_keeps_old_descriptor_state_and_filesystem() {
    let fixture = CatalogFixture::new("failed-rebind");
    let mut tabs = AppTabs::default();
    let tab_id = tabs.add_launcher();
    let mut workspaces = MarkdownVaultWorkspaceRegistry::default();
    workspaces
        .activate(&mut tabs, &fixture.catalog, tab_id, &fixture.first_id)
        .expect("activate first");
    let second_root = fixture
        .catalog
        .get(&fixture.second_id)
        .expect("second profile")
        .root
        .clone();
    std::fs::remove_dir_all(second_root).expect("remove second vault");

    let result = workspaces.request_rebind(&mut tabs, &fixture.catalog, tab_id, &fixture.second_id);

    assert!(result.is_err());
    let state = workspaces.get(tab_id).expect("old workspace");
    assert_eq!(state.profile_id(), &fixture.first_id);
    assert_eq!(
        state
            .vault_fs()
            .read_text(&CatalogFixture::note())
            .expect("read old vault"),
        "first"
    );
    assert!(tabs.tab(tab_id).is_some_and(|tab| {
        matches!(tab.view(), super::app_tabs::AppView::MarkdownVault { profile_id } if profile_id == &fixture.first_id)
    }));
}

#[test]
fn live_editor_cancel_preserves_old_workspace_and_pty() {
    let fixture = CatalogFixture::new("cancel");
    let mut tabs = AppTabs::default();
    let tab_id = tabs.add_launcher();
    let mut workspaces = MarkdownVaultWorkspaceRegistry::default();
    workspaces
        .activate(&mut tabs, &fixture.catalog, tab_id, &fixture.first_id)
        .expect("activate first");
    let (editor, probe) = live_editor();
    workspaces
        .get_mut_for_test(tab_id)
        .expect("workspace")
        .set_editor_for_test(editor);

    assert_eq!(
        workspaces
            .request_rebind(&mut tabs, &fixture.catalog, tab_id, &fixture.second_id)
            .expect("request rebind"),
        MarkdownVaultWorkspaceRebindRequest::ConfirmationRequired
    );
    assert_eq!(
        workspaces.cancel_rebind(tab_id),
        MarkdownVaultWorkspaceRebindCancel::Cancelled
    );

    assert_eq!(
        workspaces.get(tab_id).expect("workspace").profile_id(),
        &fixture.first_id
    );
    assert_eq!(probe.kills(), 0);
}

#[test]
fn live_editor_confirm_constructs_then_swaps_and_drops_once() {
    let fixture = CatalogFixture::new("confirm");
    let mut tabs = AppTabs::default();
    let tab_id = tabs.add_launcher();
    let mut workspaces = MarkdownVaultWorkspaceRegistry::default();
    workspaces
        .activate(&mut tabs, &fixture.catalog, tab_id, &fixture.first_id)
        .expect("activate first");
    let workspace_drops = Arc::new(AtomicUsize::new(0));
    let (editor, probe) = live_editor();
    let state = workspaces.get_mut_for_test(tab_id).expect("workspace");
    state.set_drop_counter_for_test(Arc::clone(&workspace_drops));
    state.set_editor_for_test(editor);
    assert_eq!(
        workspaces
            .request_rebind(&mut tabs, &fixture.catalog, tab_id, &fixture.second_id)
            .expect("request rebind"),
        MarkdownVaultWorkspaceRebindRequest::ConfirmationRequired
    );

    workspaces
        .confirm_rebind(&mut tabs, &fixture.catalog, tab_id)
        .expect("confirm rebind");

    assert_eq!(
        workspaces.get(tab_id).expect("workspace").profile_id(),
        &fixture.second_id
    );
    assert_eq!(workspace_drops.load(Ordering::SeqCst), 1);
    assert_eq!((probe.kills(), probe.waits()), (1, 1));
    assert_eq!((probe.child_drops(), probe.master_drops()), (1, 1));
}

#[test]
fn stale_app_tab_actions_are_typed_no_ops() {
    let fixture = CatalogFixture::new("stale");
    let mut tabs = AppTabs::default();
    let tab_id = tabs.add_launcher();
    let mut workspaces = MarkdownVaultWorkspaceRegistry::default();
    workspaces
        .activate(&mut tabs, &fixture.catalog, tab_id, &fixture.first_id)
        .expect("activate first");
    assert!(matches!(
        workspaces.close(&mut tabs, tab_id),
        MarkdownVaultWorkspaceRemoval::Removed { .. }
    ));

    assert_eq!(
        workspaces.poll_editor(tab_id),
        MarkdownVaultWorkspacePtyPoll::UnknownTab
    );
    assert_eq!(
        workspaces.close(&mut tabs, tab_id),
        MarkdownVaultWorkspaceRemoval::UnknownTab
    );
    assert_eq!(
        workspaces.cancel_rebind(tab_id),
        MarkdownVaultWorkspaceRebindCancel::UnknownTab
    );
}

#[test]
fn close_drops_workspace_and_live_pty_exactly_once() {
    let fixture = CatalogFixture::new("drop-once");
    let mut tabs = AppTabs::default();
    let tab_id = tabs.add_launcher();
    let mut workspaces = MarkdownVaultWorkspaceRegistry::default();
    workspaces
        .activate(&mut tabs, &fixture.catalog, tab_id, &fixture.first_id)
        .expect("activate first");
    let workspace_drops = Arc::new(AtomicUsize::new(0));
    let (editor, probe) = live_editor();
    let state = workspaces.get_mut_for_test(tab_id).expect("workspace");
    state.set_drop_counter_for_test(Arc::clone(&workspace_drops));
    state.set_editor_for_test(editor);

    let first = workspaces.close(&mut tabs, tab_id);
    let second = workspaces.close(&mut tabs, tab_id);

    assert!(matches!(
        first,
        MarkdownVaultWorkspaceRemoval::Removed { .. }
    ));
    assert_eq!(second, MarkdownVaultWorkspaceRemoval::UnknownTab);
    assert_eq!(workspace_drops.load(Ordering::SeqCst), 1);
    assert_eq!((probe.kills(), probe.waits()), (1, 1));
    assert_eq!((probe.child_drops(), probe.master_drops()), (1, 1));
}
