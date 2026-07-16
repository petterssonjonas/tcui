use std::path::PathBuf;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::{backend::TestBackend, Terminal};

use super::with_test_app_config;
use crate::config::vault_profiles::{VaultCatalog, VaultProfile};
use crate::config::AppConfig;
use crate::ui::markdown_vault::hit_areas::MarkdownVaultMouseAction;
use crate::ui::markdown_vault_workspace::MarkdownVaultFocus;

struct AppVaultFixture {
    root: PathBuf,
    config: AppConfig,
}

impl AppVaultFixture {
    fn new(label: &str) -> Self {
        let root = std::env::temp_dir().join(format!(
            "tcui-markdown-vault-app-{label}-{}-{}",
            std::process::id(),
            rand::random::<u64>()
        ));
        std::fs::create_dir_all(root.join("notes")).expect("create vault");
        std::fs::write(root.join("notes/readme.md"), "# Keyboard and mouse").expect("write note");
        let profile = VaultProfile::new("Routed Vault", &root).expect("profile");
        let profile_id = profile.id.clone();
        let mut catalog = VaultCatalog::default();
        catalog.add(profile).expect("add profile");
        let config = AppConfig {
            vaults: catalog.profiles,
            default_vault_id: Some(profile_id),
            ..AppConfig::default()
        };
        Self { root, config }
    }
}

impl Drop for AppVaultFixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

#[test]
fn markdown_vault_workspace_ui_central_keyboard_routes_enter_e_tab_and_escape_risk() {
    let fixture = AppVaultFixture::new("keys");
    with_test_app_config(
        "markdown-vault-routed-keys",
        fixture.config.clone(),
        |app| {
            app.ui.app_tabs.add_launcher();
            app.open_markdown_vault_launcher();
            let mut terminal = Terminal::new(TestBackend::new(120, 30)).expect("terminal");
            terminal.draw(|frame| app.ui.render(frame)).expect("render");
            let tab_id = app.ui.app_tabs.active_id();
            app.ui
                .markdown_vault_workspaces
                .get_mut_for_test(tab_id)
                .expect("workspace")
                .tree
                .select_path(std::path::Path::new("notes/readme.md"));

            app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
            assert_eq!(
                app.ui
                    .markdown_vault_workspaces
                    .get(tab_id)
                    .expect("workspace")
                    .focus,
                MarkdownVaultFocus::Viewer
            );
            let runtime = tokio::runtime::Runtime::new().expect("runtime");
            let _runtime_guard = runtime.enter();
            std::env::set_var("EDITOR", "/bin/true");
            app.handle_key(KeyEvent::new(KeyCode::Char('e'), KeyModifiers::NONE));
            std::env::remove_var("EDITOR");
            assert!(app
                .ui
                .markdown_vault_workspaces
                .get(tab_id)
                .is_some_and(|state| state.editor.is_some()));
            app.handle_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
            assert_eq!(
                app.ui
                    .markdown_vault_workspaces
                    .get(tab_id)
                    .expect("workspace")
                    .focus,
                MarkdownVaultFocus::ProfileControls
            );

            let (editor, _probe) =
                crate::ui::modals::terminal_editor::terminal_editor_test_support::live_editor();
            let state = app
                .ui
                .markdown_vault_workspaces
                .get_mut_for_test(tab_id)
                .expect("workspace");
            state.set_editor_for_test(editor);
            state.focus = MarkdownVaultFocus::Editor;
            app.handle_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
            assert!(app
                .ui
                .markdown_vault_workspaces
                .get(tab_id)
                .and_then(|state| state.editor.as_ref())
                .is_some_and(|editor| editor.close_is_pending()));
            app.handle_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
            assert!(app
                .ui
                .markdown_vault_workspaces
                .get(tab_id)
                .and_then(|state| state.editor.as_ref())
                .is_some_and(|editor| !editor.close_is_pending()));
        },
    );
}

#[test]
fn markdown_vault_workspace_ui_central_mouse_uses_rendered_rect_hit_areas() {
    let fixture = AppVaultFixture::new("mouse");
    with_test_app_config(
        "markdown-vault-routed-mouse",
        fixture.config.clone(),
        |app| {
            app.ui.app_tabs.add_launcher();
            app.open_markdown_vault_launcher();
            let mut terminal = Terminal::new(TestBackend::new(120, 30)).expect("terminal");
            terminal.draw(|frame| app.ui.render(frame)).expect("render");
            let tab_id = app.ui.app_tabs.active_id();
            let (row, add) = {
                let state = app
                    .ui
                    .markdown_vault_workspaces
                    .get(tab_id)
                    .expect("workspace");
                let row = state
                    .hit_areas
                    .area_for(|action| matches!(action, MarkdownVaultMouseAction::TreeRow(_)))
                    .expect("tree row area");
                let add = state
                    .hit_areas
                    .area_for(|action| matches!(action, MarkdownVaultMouseAction::AddVault))
                    .expect("add vault area");
                (row, add)
            };

            app.handle_mouse_click(row.x, row.y);
            assert_eq!(
                app.ui
                    .markdown_vault_workspaces
                    .get(tab_id)
                    .expect("workspace")
                    .focus,
                MarkdownVaultFocus::Tree
            );
            let action = app.handle_mouse_click(add.x, add.y);
            assert!(matches!(
                action,
                Some(crate::app::Action::OpenVaultProfileManager)
            ));
        },
    );
}
