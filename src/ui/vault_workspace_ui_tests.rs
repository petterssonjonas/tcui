use std::path::Path;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::{backend::TestBackend, Terminal};

use super::vault_workspace::{
    VaultContentPane, VaultFocus, VaultWorkspacePtyPoll, VaultWorkspaceRegistry,
};
use super::vault_workspace_lifecycle::NativeVaultFixture;
use super::{app_tabs::AppTabs, UI};

struct UiFixture {
    _environment: NativeVaultFixture,
}

impl UiFixture {
    fn new(label: &str) -> Self {
        let environment = NativeVaultFixture::new(label);
        let root = &environment.root;
        std::fs::create_dir_all(root.join("notes")).expect("create native notes");
        std::fs::create_dir_all(root.join(".hidden")).expect("create hidden directory");
        std::fs::write(root.join("notes/readme.md"), "# Native note").expect("write note");
        std::fs::write(root.join("payload.bin"), [0_u8, 159, 146, 150]).expect("write binary");
        std::fs::write(root.join(".hidden/ignored.txt"), "ignored").expect("write hidden file");
        Self {
            _environment: environment,
        }
    }

    fn activate(&self) -> (AppTabs, VaultWorkspaceRegistry, super::app_tabs::AppTabId) {
        let mut tabs = AppTabs::default();
        let first_tab = tabs.add_launcher();
        let mut workspaces = VaultWorkspaceRegistry::default();
        workspaces
            .activate(&mut tabs, first_tab)
            .expect("activate first");
        (tabs, workspaces, first_tab)
    }
}

fn rendered(terminal: &Terminal<TestBackend>) -> String {
    terminal
        .backend()
        .buffer()
        .content
        .iter()
        .fold(String::new(), |mut text, cell| {
            text.push_str(cell.symbol());
            text
        })
}

#[test]
fn vault_workspace_ui_tiny_terminal_keeps_minimum_size_guard() {
    let _guard = crate::test_support::env_lock()
        .lock()
        .expect("env lock poisoned");
    let _fixture = UiFixture::new("tiny");
    let mut ui = UI::new();
    let launcher = ui.app_tabs.add_launcher();
    ui.vault_workspaces
        .activate(&mut ui.app_tabs, launcher)
        .expect("activate vault");
    let mut terminal = Terminal::new(TestBackend::new(63, 15)).expect("terminal");

    terminal
        .draw(|frame| ui.render(frame))
        .expect("render tiny vault");

    assert!(rendered(&terminal).contains("TCUI requires at least 64×16"));
}

#[test]
fn vault_workspace_ui_keyboard_cycles_visible_panes_and_scrolls_focused_viewer() {
    let _guard = crate::test_support::env_lock()
        .lock()
        .expect("env lock poisoned");
    let fixture = UiFixture::new("keyboard");
    let (_tabs, mut workspaces, first_tab) = fixture.activate();
    let state = workspaces.get_mut_for_test(first_tab).expect("first state");
    state.tree.select_path(Path::new("notes/readme.md"));
    state.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
    assert_eq!(state.focus, VaultFocus::Viewer);

    state.handle_key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
    state.handle_key(KeyEvent::new(KeyCode::PageDown, KeyModifiers::NONE));
    assert!(state.viewer.scroll > 1);
    state.handle_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
    assert_eq!(state.focus, VaultFocus::Tree);
    state.handle_key(KeyEvent::new(KeyCode::BackTab, KeyModifiers::SHIFT));
    assert_eq!(state.focus, VaultFocus::Viewer);
}

#[test]
fn vault_workspace_ui_finished_editor_returns_focus_to_viewer() {
    let _guard = crate::test_support::env_lock()
        .lock()
        .expect("env lock poisoned");
    let fixture = UiFixture::new("finished-editor");
    let (_tabs, mut workspaces, first_tab) = fixture.activate();
    let (mut editor, _probe) =
        super::modals::terminal_editor::terminal_editor_test_support::live_editor();
    editor.confirm_close();
    let state = workspaces.get_mut_for_test(first_tab).expect("first state");
    state.set_editor_for_test(editor);
    state.focus = VaultFocus::Editor;
    state.content_pane = VaultContentPane::Editor;

    assert_eq!(
        workspaces.poll_editor(first_tab),
        VaultWorkspacePtyPoll::Finished
    );
    let state = workspaces.get(first_tab).expect("first state");
    assert_eq!(state.focus, VaultFocus::Viewer);
    assert_eq!(state.content_pane, VaultContentPane::Viewer);
}
