use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use super::app_tabs::AppTabs;
use super::markdown_vault_workspace::{
    MarkdownVaultInputAction, MarkdownVaultWorkspaceRegistry, MarkdownVaultWorkspaceRemoval,
};
use super::modals::terminal_editor::terminal_editor_test_support::live_editor;
use crate::config::vault_profiles::{VaultCatalog, VaultProfile, VaultProfileId};
use crate::vault::watcher::VaultWatchChange;

struct WatcherFixture {
    root: PathBuf,
    catalog: VaultCatalog,
    profile_id: VaultProfileId,
}

impl WatcherFixture {
    fn new(label: &str) -> Self {
        let root = std::env::temp_dir().join(format!(
            "tcui-workspace-watcher-{label}-{}-{}",
            std::process::id(),
            rand::random::<u64>()
        ));
        std::fs::create_dir_all(&root).expect("create watcher fixture");
        std::fs::write(root.join("a.md"), "old\nline").expect("write first note");
        std::fs::write(root.join("b.md"), "neighbor").expect("write neighbor note");
        let profile = VaultProfile::new("Watched", root.clone()).expect("profile");
        let profile_id = profile.id.clone();
        let mut catalog = VaultCatalog::default();
        catalog.add(profile).expect("add profile");
        Self {
            root,
            catalog,
            profile_id,
        }
    }

    fn activate(
        &self,
    ) -> (
        AppTabs,
        MarkdownVaultWorkspaceRegistry,
        super::app_tabs::AppTabId,
    ) {
        let mut tabs = AppTabs::default();
        let tab_id = tabs.add_launcher();
        let mut workspaces = MarkdownVaultWorkspaceRegistry::default();
        workspaces
            .activate(&mut tabs, &self.catalog, tab_id, &self.profile_id)
            .expect("activate watched workspace");
        (tabs, workspaces, tab_id)
    }
}

impl Drop for WatcherFixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

#[test]
fn vault_watcher_visible_viewer_modify_reloads_clamps_and_notices_within_five_ticks() {
    let fixture = WatcherFixture::new("viewer-modify");
    let (_tabs, mut workspaces, tab_id) = fixture.activate();
    let state = workspaces.get_mut_for_test(tab_id).expect("workspace");
    state.tree.select_path(Path::new("a.md"));
    assert_eq!(
        state.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)),
        MarkdownVaultInputAction::None
    );
    state.viewer.scroll = usize::MAX;
    std::fs::write(fixture.root.join("a.md"), "new").expect("modify viewed note");
    state.inject_watcher_event_for_test(tab_id, VaultWatchChange::Create, "a.md");
    state.inject_watcher_event_for_test(tab_id, VaultWatchChange::Modify, "a.md");

    let mut notices = Vec::new();
    let mut settled_tick = 0;
    for tick in 1..=5 {
        notices.extend(workspaces.reconcile_tick());
        if workspaces
            .get(tab_id)
            .is_some_and(|state| state.viewer_content_for_test() == Some("new"))
        {
            settled_tick = tick;
            break;
        }
    }

    let state = workspaces.get(tab_id).expect("workspace");
    assert!((1..=5).contains(&settled_tick));
    assert_eq!(state.viewer.scroll, 0);
    assert!(notices
        .iter()
        .any(|notice| notice.contains("changed on disk")));
}

#[test]
fn vault_watcher_editor_modify_marks_disk_change_without_restarting_editor() {
    let fixture = WatcherFixture::new("editor-modify");
    let (_tabs, mut workspaces, tab_id) = fixture.activate();
    let (editor, probe) = live_editor();
    let state = workspaces.get_mut_for_test(tab_id).expect("workspace");
    state.set_editor_for_test(editor);
    state.set_editor_path_for_test(PathBuf::from("a.md"));
    state.inject_watcher_event_for_test(tab_id, VaultWatchChange::Modify, "a.md");

    let notices = workspaces.reconcile_tick();

    let state = workspaces.get(tab_id).expect("workspace");
    assert!(state.editor_changed_on_disk_for_test());
    assert!(state.editor.is_some());
    assert_eq!((probe.kills(), probe.waits()), (0, 0));
    assert!(notices.iter().any(|notice| notice.contains("editor")));
}

#[test]
fn vault_watcher_selected_delete_clears_viewer_and_keeps_nearest_selection() {
    let fixture = WatcherFixture::new("delete-selected");
    let (_tabs, mut workspaces, tab_id) = fixture.activate();
    let state = workspaces.get_mut_for_test(tab_id).expect("workspace");
    state.tree.select_path(Path::new("a.md"));
    state.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
    std::fs::remove_file(fixture.root.join("a.md")).expect("remove selected note");
    state.inject_watcher_event_for_test(tab_id, VaultWatchChange::Remove, "a.md");

    let notices = workspaces.reconcile_tick();

    let state = workspaces.get(tab_id).expect("workspace");
    assert_eq!(state.tree.selected_path(), Some(Path::new("b.md")));
    assert!(state.viewer_path_for_test().is_none());
    assert!(notices.iter().any(|notice| notice.contains("removed")));
}

#[test]
fn vault_watcher_late_event_with_old_tab_id_is_ignored() {
    let fixture = WatcherFixture::new("stale-tab");
    let mut tabs = AppTabs::default();
    let old_tab = tabs.add_launcher();
    let new_tab = tabs.add_launcher();
    let mut workspaces = MarkdownVaultWorkspaceRegistry::default();
    workspaces
        .activate(&mut tabs, &fixture.catalog, new_tab, &fixture.profile_id)
        .expect("activate current workspace");
    let state = workspaces.get_mut_for_test(new_tab).expect("workspace");
    state.inject_watcher_event_for_test(old_tab, VaultWatchChange::Remove, "a.md");

    let notices = workspaces.reconcile_tick();

    assert!(notices.is_empty());
    assert!(workspaces
        .get(new_tab)
        .expect("workspace")
        .tree
        .nodes()
        .iter()
        .any(|node| node.path == Path::new("a.md")));
}

#[test]
fn vault_watcher_overflow_rescans_then_clears_flag_after_success() {
    let fixture = WatcherFixture::new("overflow-rescan");
    let (_tabs, mut workspaces, tab_id) = fixture.activate();
    workspaces
        .get(tab_id)
        .expect("workspace")
        .force_watcher_overflow_for_test();

    let notices = workspaces.reconcile_tick();

    let state = workspaces.get(tab_id).expect("workspace");
    assert!(!state.watcher_overflowed_for_test());
    assert!(notices
        .iter()
        .any(|notice| notice.contains("full rescan started")));
}

#[test]
fn vault_watcher_close_and_rebind_drop_each_watcher_exactly_once() {
    let fixture = WatcherFixture::new("drop-counts");
    let (mut tabs, mut workspaces, tab_id) = fixture.activate();
    let close_drops = Arc::new(AtomicUsize::new(0));
    workspaces
        .get_mut_for_test(tab_id)
        .expect("workspace")
        .set_watcher_drop_counter_for_test(Arc::clone(&close_drops));

    assert!(matches!(
        workspaces.close(&mut tabs, tab_id),
        MarkdownVaultWorkspaceRemoval::Removed { .. }
    ));
    assert_eq!(close_drops.load(Ordering::SeqCst), 1);

    let (mut tabs, mut workspaces, tab_id) = fixture.activate();
    let replacement_root = fixture.root.join("replacement");
    std::fs::create_dir_all(&replacement_root).expect("create replacement vault");
    let replacement = VaultProfile::new("Replacement", replacement_root).expect("replacement");
    let replacement_id = replacement.id.clone();
    let mut catalog = fixture.catalog.clone();
    catalog.add(replacement).expect("add replacement");
    let rebind_drops = Arc::new(AtomicUsize::new(0));
    workspaces
        .get_mut_for_test(tab_id)
        .expect("workspace")
        .set_watcher_drop_counter_for_test(Arc::clone(&rebind_drops));

    workspaces
        .request_rebind(&mut tabs, &catalog, tab_id, &replacement_id)
        .expect("rebind workspace");

    assert_eq!(rebind_drops.load(Ordering::SeqCst), 1);
}
