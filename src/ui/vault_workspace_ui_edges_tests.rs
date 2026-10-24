use std::path::{Path, PathBuf};

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::{backend::TestBackend, layout::Position, Terminal};

use super::vault::hit_areas::VaultMouseAction;
use super::vault_workspace::{VaultFocus, VaultInputAction, VaultWorkspaceRegistry};
use super::vault_workspace_lifecycle::NativeVaultFixture;
use super::{app_tabs::AppTabs, UI};

struct EdgeFixture {
    _environment: NativeVaultFixture,
}

impl EdgeFixture {
    fn new(label: &str) -> Self {
        let environment = NativeVaultFixture::new(label);
        let root = &environment.root;
        std::fs::create_dir_all(root.join("notes")).expect("create vault");
        std::fs::write(root.join("notes/readme.md"), "# Edge note").expect("write note");
        Self {
            _environment: environment,
        }
    }

    fn workspace(&self) -> (AppTabs, VaultWorkspaceRegistry, super::app_tabs::AppTabId) {
        let mut tabs = AppTabs::default();
        let tab_id = tabs.add_launcher();
        let mut workspaces = VaultWorkspaceRegistry::default();
        workspaces
            .activate(&mut tabs, tab_id)
            .expect("activate workspace");
        (tabs, workspaces, tab_id)
    }
}

#[test]
fn vault_workspace_ui_enter_collapses_expands_and_opens_tree_nodes() {
    let _guard = crate::test_support::env_lock()
        .lock()
        .expect("env lock poisoned");
    let fixture = EdgeFixture::new("tree-enter");
    let (_tabs, mut workspaces, tab_id) = fixture.workspace();
    let state = workspaces.get_mut_for_test(tab_id).expect("workspace");
    state.tree.select_path(Path::new("notes"));

    state.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
    assert!(!state
        .tree
        .nodes()
        .iter()
        .any(|node| node.path == Path::new("notes/readme.md")));
    state.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
    assert!(state
        .tree
        .nodes()
        .iter()
        .any(|node| node.path == Path::new("notes/readme.md")));
    state.tree.select_path(Path::new("notes/readme.md"));
    assert_eq!(
        state.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)),
        VaultInputAction::None
    );
    assert_eq!(state.focus, VaultFocus::Viewer);
}

#[test]
fn vault_workspace_ui_read_failure_is_localized_to_notice() {
    let _guard = crate::test_support::env_lock()
        .lock()
        .expect("env lock poisoned");
    let fixture = EdgeFixture::new("read-failure");
    let (_tabs, mut workspaces, tab_id) = fixture.workspace();
    let state = workspaces.get_mut_for_test(tab_id).expect("workspace");
    state.tree.select_path(Path::new("notes/readme.md"));
    std::fs::remove_file(state.root.join("notes/readme.md")).expect("remove note");

    let action = state.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));

    assert!(matches!(action, VaultInputAction::Notice(_)));
    assert_eq!(state.focus, VaultFocus::Tree);
}

#[test]
fn vault_workspace_ui_move_input_emits_selected_source_and_typed_destination() {
    // Given
    let _guard = crate::test_support::env_lock()
        .lock()
        .expect("env lock poisoned");
    let fixture = EdgeFixture::new("move-input");
    let (_tabs, mut workspaces, tab_id) = fixture.workspace();
    let state = workspaces.get_mut_for_test(tab_id).expect("workspace");
    state.tree.select_path(Path::new("notes/readme.md"));

    // When
    state.handle_key(KeyEvent::new(KeyCode::Char('m'), KeyModifiers::NONE));
    assert_eq!(state.move_input_value_for_test(), Some("notes/readme.md"));
    for character in "archive/readme.md".chars() {
        state.handle_key(KeyEvent::new(KeyCode::Char(character), KeyModifiers::NONE));
    }
    let action = state.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));

    // Then
    assert_eq!(
        action,
        VaultInputAction::Move {
            source: PathBuf::from("notes/readme.md"),
            destination: PathBuf::from("archive/readme.md"),
        }
    );
}

#[test]
fn vault_workspace_ui_rename_input_keeps_destination_in_source_directory() {
    // Given
    let _guard = crate::test_support::env_lock()
        .lock()
        .expect("env lock poisoned");
    let fixture = EdgeFixture::new("rename-input");
    let (_tabs, mut workspaces, tab_id) = fixture.workspace();
    let state = workspaces.get_mut_for_test(tab_id).expect("workspace");
    state.tree.select_path(Path::new("notes/readme.md"));

    // When
    state.handle_key(KeyEvent::new(KeyCode::Char('r'), KeyModifiers::NONE));
    for character in "renamed.md".chars() {
        state.handle_key(KeyEvent::new(KeyCode::Char(character), KeyModifiers::NONE));
    }
    let action = state.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));

    // Then
    assert_eq!(
        action,
        VaultInputAction::Move {
            source: PathBuf::from("notes/readme.md"),
            destination: PathBuf::from("notes/renamed.md"),
        }
    );
}

#[test]
fn vault_workspace_ui_move_input_escape_cancels_without_action() {
    // Given
    let _guard = crate::test_support::env_lock()
        .lock()
        .expect("env lock poisoned");
    let fixture = EdgeFixture::new("move-cancel");
    let (_tabs, mut workspaces, tab_id) = fixture.workspace();
    let state = workspaces.get_mut_for_test(tab_id).expect("workspace");
    state.tree.select_path(Path::new("notes/readme.md"));
    state.handle_key(KeyEvent::new(KeyCode::Char('m'), KeyModifiers::NONE));

    // When
    let action = state.handle_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));

    // Then
    assert_eq!(action, VaultInputAction::None);
    assert!(state.move_input_value_for_test().is_none());
}

#[test]
fn vault_workspace_ui_move_input_blocks_underlying_mouse_and_scroll() {
    // Given
    let _guard = crate::test_support::env_lock()
        .lock()
        .expect("env lock poisoned");
    let fixture = EdgeFixture::new("move-mouse-capture");
    let (_tabs, mut workspaces, tab_id) = fixture.workspace();
    let state = workspaces.get_mut_for_test(tab_id).expect("workspace");
    state.tree.select_path(Path::new("notes/readme.md"));
    state.handle_key(KeyEvent::new(KeyCode::Char('m'), KeyModifiers::NONE));
    let selected = state.tree.selected_path().map(Path::to_path_buf);

    // When
    let action = state.handle_mouse(VaultMouseAction::TreeRow(0));
    let scrolled = state.scroll_at(Position::new(0, 0), true);

    // Then
    assert_eq!(action, VaultInputAction::None);
    assert!(!scrolled);
    assert_eq!(state.tree.selected_path(), selected.as_deref());
    assert_eq!(state.move_input_value_for_test(), Some("notes/readme.md"));
}

#[test]
fn vault_workspace_ui_tiny_guard_clears_wide_hit_areas() {
    let _guard = crate::test_support::env_lock()
        .lock()
        .expect("env lock poisoned");
    let _fixture = EdgeFixture::new("stale-hit-areas");
    let mut ui = UI::new();
    let tab_id = ui.app_tabs.add_launcher();
    ui.vault_workspaces
        .activate(&mut ui.app_tabs, tab_id)
        .expect("activate workspace");
    let mut wide = Terminal::new(TestBackend::new(120, 30)).expect("wide terminal");
    wide.draw(|frame| ui.render(frame)).expect("render wide");
    let old_row = ui
        .vault_workspaces
        .get(tab_id)
        .and_then(|state| {
            state
                .hit_areas
                .area_for(|action| matches!(action, VaultMouseAction::TreeRow(_)))
        })
        .expect("tree row");
    let mut tiny = Terminal::new(TestBackend::new(63, 15)).expect("tiny terminal");

    tiny.draw(|frame| ui.render(frame)).expect("render tiny");

    assert!(ui
        .vault_workspaces
        .get(tab_id)
        .and_then(|state| {
            state
                .hit_areas
                .action_at(Position::new(old_row.x, old_row.y))
        })
        .is_none());
}
