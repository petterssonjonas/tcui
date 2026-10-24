use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use super::app_tabs::AppTabs;
use super::modals::terminal_editor::terminal_editor_test_support::live_editor;
use super::vault_workspace::{VaultInputAction, VaultWorkspaceRegistry, VaultWorkspaceRemoval};
use super::vault_workspace_lifecycle::NativeVaultFixture;
use crate::vault::watcher::VaultWatchChange;

struct WatcherFixture {
    root: PathBuf,
    _environment: NativeVaultFixture,
}

impl WatcherFixture {
    fn new(label: &str) -> Self {
        let environment = NativeVaultFixture::new(label);
        let root = environment.root.clone();
        std::fs::write(root.join("a.md"), "old\nline").expect("write first note");
        std::fs::write(root.join("b.md"), "neighbor").expect("write neighbor note");
        Self {
            root,
            _environment: environment,
        }
    }

    fn activate(&self) -> (AppTabs, VaultWorkspaceRegistry, super::app_tabs::AppTabId) {
        let mut tabs = AppTabs::default();
        let tab_id = tabs.add_launcher();
        let mut workspaces = VaultWorkspaceRegistry::default();
        workspaces
            .activate(&mut tabs, tab_id)
            .expect("activate watched workspace");
        (tabs, workspaces, tab_id)
    }
}

#[test]
fn vault_watcher_visible_viewer_modify_reloads_clamps_and_notices_within_five_ticks() {
    let _guard = crate::test_support::env_lock()
        .lock()
        .expect("env lock poisoned");
    let fixture = WatcherFixture::new("viewer-modify");
    let (_tabs, mut workspaces, tab_id) = fixture.activate();
    let state = workspaces.get_mut_for_test(tab_id).expect("workspace");
    state.tree.select_path(Path::new("a.md"));
    assert_eq!(
        state.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)),
        VaultInputAction::None
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
    let _guard = crate::test_support::env_lock()
        .lock()
        .expect("env lock poisoned");
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
    let _guard = crate::test_support::env_lock()
        .lock()
        .expect("env lock poisoned");
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
    let _guard = crate::test_support::env_lock()
        .lock()
        .expect("env lock poisoned");
    let _fixture = WatcherFixture::new("stale-tab");
    let mut tabs = AppTabs::default();
    let old_tab = tabs.add_launcher();
    let new_tab = tabs.add_launcher();
    let mut workspaces = VaultWorkspaceRegistry::default();
    workspaces
        .activate(&mut tabs, new_tab)
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
    let _guard = crate::test_support::env_lock()
        .lock()
        .expect("env lock poisoned");
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
fn vault_watcher_close_drops_watcher_exactly_once() {
    let _guard = crate::test_support::env_lock()
        .lock()
        .expect("env lock poisoned");
    let fixture = WatcherFixture::new("drop-counts");
    let (mut tabs, mut workspaces, tab_id) = fixture.activate();
    let close_drops = Arc::new(AtomicUsize::new(0));
    workspaces
        .get_mut_for_test(tab_id)
        .expect("workspace")
        .set_watcher_drop_counter_for_test(Arc::clone(&close_drops));

    assert!(matches!(
        workspaces.close(&mut tabs, tab_id),
        VaultWorkspaceRemoval::Removed { .. }
    ));
    assert_eq!(close_drops.load(Ordering::SeqCst), 1);
}
