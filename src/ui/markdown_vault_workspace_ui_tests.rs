use std::path::{Path, PathBuf};

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::{backend::TestBackend, Terminal};

use super::markdown_vault_workspace::{
    MarkdownVaultContentPane, MarkdownVaultFocus, MarkdownVaultInputAction,
    MarkdownVaultWorkspacePtyPoll, MarkdownVaultWorkspaceRegistry,
};
use super::{app_tabs::AppTabs, UI};
use crate::config::vault_profiles::{VaultCatalog, VaultProfile, VaultProfileId};

struct UiFixture {
    root: PathBuf,
    catalog: VaultCatalog,
    first_id: VaultProfileId,
    second_id: VaultProfileId,
}

impl UiFixture {
    fn new(label: &str) -> Self {
        let root = std::env::temp_dir().join(format!(
            "tcui-markdown-vault-ui-{label}-{}-{}",
            std::process::id(),
            rand::random::<u64>()
        ));
        let first_root = root.join("first");
        let second_root = root.join("second");
        std::fs::create_dir_all(first_root.join("notes")).expect("create first vault");
        std::fs::create_dir_all(first_root.join(".obsidian/plugins/demo"))
            .expect("create obsidian plugin");
        std::fs::create_dir_all(&second_root).expect("create second vault");
        std::fs::write(first_root.join("notes/readme.md"), "# First vault")
            .expect("write first note");
        std::fs::write(first_root.join("payload.bin"), [0_u8, 159, 146, 150])
            .expect("write binary");
        std::fs::write(first_root.join(".obsidian/plugins/demo/main.js"), "ignored")
            .expect("write plugin");
        std::fs::write(second_root.join("second.md"), "# Second vault").expect("write second note");
        let first = VaultProfile::new("First Vault", first_root).expect("first profile");
        let second = VaultProfile::new("Second Vault", second_root).expect("second profile");
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

    fn activate_two(
        &self,
    ) -> (
        AppTabs,
        MarkdownVaultWorkspaceRegistry,
        super::app_tabs::AppTabId,
        super::app_tabs::AppTabId,
    ) {
        let mut tabs = AppTabs::default();
        let first_tab = tabs.add_launcher();
        let second_tab = tabs.add_launcher();
        let mut workspaces = MarkdownVaultWorkspaceRegistry::default();
        workspaces
            .activate(&mut tabs, &self.catalog, first_tab, &self.first_id)
            .expect("activate first");
        workspaces
            .activate(&mut tabs, &self.catalog, second_tab, &self.second_id)
            .expect("activate second");
        (tabs, workspaces, first_tab, second_tab)
    }
}

impl Drop for UiFixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
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
fn markdown_vault_workspace_ui_two_tabs_keep_tree_viewer_and_editor_state_independent() {
    let fixture = UiFixture::new("independent-state");
    let (_tabs, mut workspaces, first_tab, second_tab) = fixture.activate_two();
    let first = workspaces.get_mut_for_test(first_tab).expect("first state");
    first.tree.select_path(Path::new("notes/readme.md"));
    assert_eq!(
        first.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)),
        MarkdownVaultInputAction::None
    );
    first.viewer.scroll = 7;
    let (editor, _probe) =
        super::modals::terminal_editor::terminal_editor_test_support::live_editor();
    first.set_editor_for_test(editor);
    let first_selected = first.tree.selected_path().map(Path::to_path_buf);

    let second = workspaces.get(second_tab).expect("second state");
    assert_ne!(first_selected.as_deref(), second.tree.selected_path());
    assert_eq!(second.viewer.scroll, 0);
    assert!(second.editor.is_none());
}

#[test]
fn markdown_vault_workspace_ui_binary_metadata_is_visible_and_edit_is_blocked() {
    let fixture = UiFixture::new("binary");
    let (_tabs, mut workspaces, first_tab, _second_tab) = fixture.activate_two();
    let state = workspaces.get_mut_for_test(first_tab).expect("first state");
    state.tree.select_path(Path::new("payload.bin"));

    assert_eq!(
        state.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)),
        MarkdownVaultInputAction::None
    );
    assert!(state
        .metadata()
        .is_some_and(|metadata| metadata.contains("Binary")));
    let edit = state.handle_key(KeyEvent::new(KeyCode::Char('e'), KeyModifiers::NONE));
    assert!(matches!(edit, MarkdownVaultInputAction::Notice(_)));
    assert!(state.editor.is_none());
}

#[test]
fn markdown_vault_workspace_ui_render_replaces_chat_chrome_but_keeps_top_bar_and_footer() {
    let fixture = UiFixture::new("render");
    let mut ui = UI::new();
    let launcher = ui.app_tabs.add_launcher();
    ui.markdown_vault_workspaces
        .activate(
            &mut ui.app_tabs,
            &fixture.catalog,
            launcher,
            &fixture.first_id,
        )
        .expect("activate vault");
    let tab_id = ui.app_tabs.active_id();
    ui.markdown_vault_workspaces
        .get_mut_for_test(tab_id)
        .expect("workspace")
        .tree
        .select_path(Path::new("notes/readme.md"));
    ui.show_toast("vault notice".to_string());
    ui.active_modal = Some(super::Modal::QuitConfirm);
    let mut terminal = Terminal::new(TestBackend::new(120, 30)).expect("terminal");

    terminal
        .draw(|frame| ui.render(frame))
        .expect("render vault");
    let text = rendered(&terminal);

    assert!(text.contains("Markdown Vault"));
    assert!(text.contains("First Vault"));
    assert!(text.contains("+ Add vault"));
    assert!(text.contains("m Move  r Rename"));
    assert!(text.contains("readme.md"));
    assert!(text.contains("vault notice"));
    assert!(ui.modal_areas.is_some());
    assert!(ui.chat_area.is_none());
    assert!(ui.status_bar_areas.is_none());
    assert!(!text.contains("Chat files"));
}

#[test]
fn markdown_vault_workspace_ui_tiny_terminal_keeps_minimum_size_guard() {
    let fixture = UiFixture::new("tiny");
    let mut ui = UI::new();
    let launcher = ui.app_tabs.add_launcher();
    ui.markdown_vault_workspaces
        .activate(
            &mut ui.app_tabs,
            &fixture.catalog,
            launcher,
            &fixture.first_id,
        )
        .expect("activate vault");
    let mut terminal = Terminal::new(TestBackend::new(63, 15)).expect("terminal");

    terminal
        .draw(|frame| ui.render(frame))
        .expect("render tiny vault");

    assert!(rendered(&terminal).contains("TCUI requires at least 64×16"));
}

#[test]
fn markdown_vault_workspace_ui_keyboard_cycles_visible_panes_and_scrolls_focused_viewer() {
    let fixture = UiFixture::new("keyboard");
    let (_tabs, mut workspaces, first_tab, _second_tab) = fixture.activate_two();
    let state = workspaces.get_mut_for_test(first_tab).expect("first state");
    state.tree.select_path(Path::new("notes/readme.md"));
    state.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
    assert_eq!(state.focus, MarkdownVaultFocus::Viewer);

    state.handle_key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
    state.handle_key(KeyEvent::new(KeyCode::PageDown, KeyModifiers::NONE));
    assert!(state.viewer.scroll > 1);
    state.handle_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
    assert_eq!(state.focus, MarkdownVaultFocus::ProfileControls);
    state.handle_key(KeyEvent::new(KeyCode::BackTab, KeyModifiers::SHIFT));
    assert_eq!(state.focus, MarkdownVaultFocus::Viewer);
}

#[test]
fn markdown_vault_workspace_ui_finished_editor_returns_focus_to_viewer() {
    let fixture = UiFixture::new("finished-editor");
    let (_tabs, mut workspaces, first_tab, _second_tab) = fixture.activate_two();
    let (mut editor, _probe) =
        super::modals::terminal_editor::terminal_editor_test_support::live_editor();
    editor.confirm_close();
    let state = workspaces.get_mut_for_test(first_tab).expect("first state");
    state.set_editor_for_test(editor);
    state.focus = MarkdownVaultFocus::Editor;
    state.content_pane = MarkdownVaultContentPane::Editor;

    assert_eq!(
        workspaces.poll_editor(first_tab),
        MarkdownVaultWorkspacePtyPoll::Finished
    );
    let state = workspaces.get(first_tab).expect("first state");
    assert_eq!(state.focus, MarkdownVaultFocus::Viewer);
    assert_eq!(state.content_pane, MarkdownVaultContentPane::Viewer);
}

#[test]
fn markdown_vault_workspace_ui_profile_dropdown_routes_selected_rebind() {
    let fixture = UiFixture::new("profile-dropdown");
    let (_tabs, mut workspaces, first_tab, _second_tab) = fixture.activate_two();
    let state = workspaces.get_mut_for_test(first_tab).expect("first state");
    state.focus = MarkdownVaultFocus::ProfileControls;

    state.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
    state.handle_key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
    let action = state.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));

    assert_eq!(
        action,
        MarkdownVaultInputAction::Rebind(fixture.second_id.clone())
    );
}
