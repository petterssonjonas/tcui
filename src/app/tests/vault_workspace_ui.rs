use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::{backend::TestBackend, Terminal};

use super::with_test_app_config;
use crate::config::AppConfig;
use crate::ui::vault::hit_areas::VaultMouseAction;
use crate::ui::vault_workspace::VaultFocus;

struct AppVaultFixture {
    config: AppConfig,
}

impl AppVaultFixture {
    fn new(_label: &str) -> Self {
        Self {
            config: AppConfig::default(),
        }
    }
}

fn open_configured_workspace(app: &mut crate::app::TuiApp) {
    let root = crate::protection::internal_vault_root();
    std::fs::create_dir_all(root.join("notes")).expect("create internal Vault");
    std::fs::write(root.join("notes/readme.md"), "# Keyboard and mouse").expect("write note");
    app.ui.app_tabs.add_launcher();
    app.open_vault_launcher();
}

#[test]
fn command_popup_opens_vault_from_chat_without_a_launcher() {
    with_test_app_config("palette-vault-from-chat", AppConfig::default(), |app| {
        let root = crate::protection::internal_vault_root();
        std::fs::create_dir_all(&root).expect("create internal Vault");
        let runtime = tokio::runtime::Runtime::new().expect("runtime");
        let action = app
            .handle_key(KeyEvent::new(KeyCode::Char('p'), KeyModifiers::CONTROL))
            .expect("open popup action");
        runtime.block_on(app.dispatch(action)).expect("open popup");
        for character in "vault workspace".chars() {
            app.handle_key(KeyEvent::new(KeyCode::Char(character), KeyModifiers::NONE));
        }
        let action = app
            .handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE))
            .expect("vault destination action");
        runtime.block_on(app.dispatch(action)).expect("open Vault");
        assert_eq!(
            app.ui.app_tabs.active_view(),
            crate::ui::app_tabs::AppView::Vault
        );
        assert!(app
            .ui
            .vault_workspaces
            .get(app.ui.app_tabs.active_id())
            .is_some());
        for (query, expected) in [
            ("launcher", crate::ui::app_tabs::AppView::Launcher),
            ("focus input", crate::ui::app_tabs::AppView::Chat),
            ("vault workspace", crate::ui::app_tabs::AppView::Vault),
        ] {
            let tabs_before = app.ui.app_tabs.tabs().len();
            let action = app
                .handle_key(KeyEvent::new(KeyCode::Char('p'), KeyModifiers::CONTROL))
                .expect("reopen popup action");
            runtime.block_on(app.dispatch(action)).expect("open popup");
            for character in query.chars() {
                app.handle_key(KeyEvent::new(KeyCode::Char(character), KeyModifiers::NONE));
            }
            let action = app
                .handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE))
                .expect("destination action");
            runtime.block_on(app.dispatch(action)).expect("navigate");
            assert_eq!(app.ui.app_tabs.active_view(), expected);
            let added_tabs = usize::from(expected == crate::ui::app_tabs::AppView::Launcher);
            assert_eq!(app.ui.app_tabs.tabs().len(), tabs_before + added_tabs);
        }
    });
}

#[test]
fn vault_workspace_ui_central_keyboard_routes_enter_e_tab_and_escape_risk() {
    let fixture = AppVaultFixture::new("keys");
    with_test_app_config(
        "markdown-vault-routed-keys",
        fixture.config.clone(),
        |app| {
            open_configured_workspace(app);
            let mut terminal = Terminal::new(TestBackend::new(120, 30)).expect("terminal");
            terminal.draw(|frame| app.ui.render(frame)).expect("render");
            let tab_id = app.ui.app_tabs.active_id();
            app.ui
                .vault_workspaces
                .get_mut_for_test(tab_id)
                .expect("workspace")
                .tree
                .select_path(std::path::Path::new("notes/readme.md"));

            app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
            assert_eq!(
                app.ui
                    .vault_workspaces
                    .get(tab_id)
                    .expect("workspace")
                    .focus,
                VaultFocus::Viewer
            );
            let runtime = tokio::runtime::Runtime::new().expect("runtime");
            let _runtime_guard = runtime.enter();
            std::env::set_var("EDITOR", "/bin/true");
            app.handle_key(KeyEvent::new(KeyCode::Char('e'), KeyModifiers::NONE));
            std::env::remove_var("EDITOR");
            assert!(app
                .ui
                .vault_workspaces
                .get(tab_id)
                .is_some_and(|state| state.editor.is_some()));
            app.handle_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
            assert_eq!(
                app.ui
                    .vault_workspaces
                    .get(tab_id)
                    .expect("workspace")
                    .focus,
                VaultFocus::Tree
            );

            let (editor, _probe) =
                crate::ui::modals::terminal_editor::terminal_editor_test_support::live_editor();
            let state = app
                .ui
                .vault_workspaces
                .get_mut_for_test(tab_id)
                .expect("workspace");
            state.set_editor_for_test(editor);
            state.focus = VaultFocus::Editor;
            app.handle_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
            assert!(app
                .ui
                .vault_workspaces
                .get(tab_id)
                .and_then(|state| state.editor.as_ref())
                .is_some_and(|editor| editor.close_is_pending()));
            app.handle_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
            assert!(app
                .ui
                .vault_workspaces
                .get(tab_id)
                .and_then(|state| state.editor.as_ref())
                .is_some_and(|editor| !editor.close_is_pending()));
        },
    );
}

#[test]
fn security_transition_requires_consent_for_an_unfocused_vault_editor() {
    let fixture = AppVaultFixture::new("security-editor-consent");
    with_test_app_config(
        "security-vault-editor-consent",
        fixture.config.clone(),
        |app| {
            open_configured_workspace(app);
            let tab_id = app.ui.app_tabs.active_id();
            let (editor, probe) =
                crate::ui::modals::terminal_editor::terminal_editor_test_support::live_editor();
            app.ui
                .vault_workspaces
                .get_mut_for_test(tab_id)
                .expect("workspace")
                .set_editor_for_test(editor);
            app.focus_chat_pane(0);
            let runtime = tokio::runtime::Runtime::new().expect("runtime");
            app.unlock_application();
            assert_eq!(
                app.ui.active_modal,
                Some(crate::ui::Modal::DiscardSecurityDrafts)
            );
            assert!(app.security_restart.is_none());
            let cancel = app
                .handle_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE))
                .expect("cancel action");
            runtime.block_on(app.dispatch(cancel)).expect("cancel");
            assert!(app.security_restart.is_none());
            assert!(!app.ui.protection_blocked);
            assert_eq!(probe.kills(), 0);
            assert!(app
                .ui
                .vault_workspaces
                .get(tab_id)
                .and_then(|state| state.editor.as_ref())
                .is_some_and(|editor| editor.has_unsaved_risk() && !editor.close_is_pending()));

            app.unlock_application();
            let confirm = app
                .handle_key(KeyEvent::new(KeyCode::Char('y'), KeyModifiers::NONE))
                .expect("confirm action");
            runtime.block_on(app.dispatch(confirm)).expect("confirm");
            assert_eq!(
                app.security_restart,
                Some(crate::protection::ui::SecurityRestart::Unlock)
            );
            runtime
                .block_on(app.quiesce_for_security_restart())
                .expect("quiesce");
            // The security restart recreates TuiApp, dropping this separately owned registry.
            drop(std::mem::take(&mut app.ui.vault_workspaces));
            assert_eq!(probe.kills(), 1);
            assert_eq!(probe.waits(), 1);
        },
    );
}

#[test]
fn revoked_key_security_restart_does_not_offer_vault_editor_cancellation() {
    let fixture = AppVaultFixture::new("security-editor-revoked");
    with_test_app_config(
        "security-vault-editor-revoked",
        fixture.config.clone(),
        |app| {
            open_configured_workspace(app);
            let tab_id = app.ui.app_tabs.active_id();
            let (editor, probe) =
                crate::ui::modals::terminal_editor::terminal_editor_test_support::live_editor();
            app.ui
                .vault_workspaces
                .get_mut_for_test(tab_id)
                .expect("workspace")
                .set_editor_for_test(editor);
            app.unlock_application();
            assert!(app.ui.active_modal.is_some());
            app.lock_application_after_error("revoked test key".into());
            assert!(app.ui.active_modal.is_none());
            assert!(app.pending_document_discard_action.is_none());
            assert!(app.ui.protection_blocked);
            assert_eq!(
                app.security_restart,
                Some(crate::protection::ui::SecurityRestart::Lock)
            );
            assert!(app.security_notice.is_some());
            drop(std::mem::take(&mut app.ui.vault_workspaces));
            assert_eq!(probe.kills(), 1);
            assert_eq!(probe.waits(), 1);
        },
    );
}

#[test]
fn vault_workspace_ui_central_mouse_uses_rendered_rect_hit_areas() {
    let fixture = AppVaultFixture::new("mouse");
    with_test_app_config(
        "markdown-vault-routed-mouse",
        fixture.config.clone(),
        |app| {
            open_configured_workspace(app);
            let mut terminal = Terminal::new(TestBackend::new(120, 30)).expect("terminal");
            terminal.draw(|frame| app.ui.render(frame)).expect("render");
            let tab_id = app.ui.app_tabs.active_id();
            let row = {
                let state = app.ui.vault_workspaces.get(tab_id).expect("workspace");
                state
                    .hit_areas
                    .area_for(|action| matches!(action, VaultMouseAction::TreeRow(_)))
                    .expect("tree row area")
            };

            app.handle_mouse_click(row.x, row.y);
            assert_eq!(
                app.ui
                    .vault_workspaces
                    .get(tab_id)
                    .expect("workspace")
                    .focus,
                VaultFocus::Tree
            );
        },
    );
}

#[test]
fn vault_workspace_ui_move_previews_cancels_and_executes_with_tree_refresh() {
    // Given
    let fixture = AppVaultFixture::new("move-flow");
    with_test_app_config("markdown-vault-move-flow", fixture.config.clone(), |app| {
        open_configured_workspace(app);
        let tab_id = app.ui.app_tabs.active_id();
        app.ui
            .vault_workspaces
            .get_mut_for_test(tab_id)
            .expect("workspace")
            .tree
            .select_path(std::path::Path::new("notes/readme.md"));

        app.handle_key(KeyEvent::new(KeyCode::Char('m'), KeyModifiers::NONE));
        let mut terminal = Terminal::new(TestBackend::new(120, 30)).expect("terminal");
        terminal
            .draw(|frame| app.ui.render(frame))
            .expect("render move input");
        let rendered = terminal.backend().to_string();
        assert!(rendered.contains("Move file"));
        assert!(rendered.contains("notes/readme.md"));
        app.handle_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));

        let enter_move = |app: &mut crate::app::TuiApp| {
            app.handle_key(KeyEvent::new(KeyCode::Char('m'), KeyModifiers::NONE));
            for character in "archive/readme.md".chars() {
                app.handle_key(KeyEvent::new(KeyCode::Char(character), KeyModifiers::NONE));
            }
            app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        };

        // When: preview then cancel
        enter_move(app);

        // Then: preview is visible and cancellation leaves disk untouched
        let pending = app
            .ui
            .pending_vault_mutation
            .as_ref()
            .expect("workspace move preview");
        assert!(pending
            .preview
            .rows
            .iter()
            .any(|row| row.contains("archive/readme.md")));
        app.cancel_pending_vault_mutation()
            .expect("cancel workspace move");
        assert!(crate::protection::internal_vault_root()
            .join("notes/readme.md")
            .exists());
        assert!(!crate::protection::internal_vault_root()
            .join("archive/readme.md")
            .exists());

        // When: preview then confirm
        enter_move(app);
        let runtime = tokio::runtime::Runtime::new().expect("runtime");
        runtime
            .block_on(app.confirm_pending_vault_mutation())
            .expect("confirm workspace move");

        // Then: disk and workspace tree both reflect the move
        assert!(!crate::protection::internal_vault_root()
            .join("notes/readme.md")
            .exists());
        assert!(crate::protection::internal_vault_root()
            .join("archive/readme.md")
            .exists());
        let workspace = app
            .ui
            .vault_workspaces
            .get(tab_id)
            .expect("workspace after move");
        assert!(workspace
            .tree
            .nodes()
            .iter()
            .any(|node| node.path == std::path::Path::new("archive/readme.md")));
        assert!(app.ui.pending_vault_mutation.is_none());
    });
}

#[test]
fn vault_workspace_ui_move_collision_disables_confirmation() {
    // Given
    let fixture = AppVaultFixture::new("move-collision");
    with_test_app_config(
        "markdown-vault-move-collision",
        fixture.config.clone(),
        |app| {
            open_configured_workspace(app);
            let root = crate::protection::internal_vault_root();
            std::fs::create_dir_all(root.join("archive")).expect("create destination directory");
            std::fs::write(root.join("archive/readme.md"), "existing").expect("write collision");
            let tab_id = app.ui.app_tabs.active_id();
            app.ui
                .vault_workspaces
                .get_mut_for_test(tab_id)
                .expect("workspace")
                .tree
                .select_path(std::path::Path::new("notes/readme.md"));

            // When
            app.handle_key(KeyEvent::new(KeyCode::Char('m'), KeyModifiers::NONE));
            for character in "archive/readme.md".chars() {
                app.handle_key(KeyEvent::new(KeyCode::Char(character), KeyModifiers::NONE));
            }
            app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));

            // Then
            let pending = app
                .ui
                .pending_vault_mutation
                .as_ref()
                .expect("collision preview");
            assert!(!pending.preview.confirm_enabled);
            assert!(pending
                .preview
                .rows
                .iter()
                .any(|row| row.contains("unapproved collision")));
        },
    );
}

#[test]
fn closing_vault_tab_requires_editor_discard_confirmation() {
    with_test_app_config("vault-tab-close-consent", AppConfig::default(), |app| {
        open_configured_workspace(app);
        let tab_id = app.ui.app_tabs.active_id();
        let (editor, probe) =
            crate::ui::modals::terminal_editor::terminal_editor_test_support::live_editor();
        app.ui
            .vault_workspaces
            .get_mut_for_test(tab_id)
            .expect("workspace")
            .set_editor_for_test(editor);
        app.request_app_tab_close(tab_id);
        assert!(app.ui.app_tabs.tab(tab_id).is_some());
        assert_eq!(probe.kills(), 0);

        app.handle_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
        assert!(app.ui.pending_vault_action.is_none());
        assert!(app.ui.app_tabs.tab(tab_id).is_some());
        assert_eq!(probe.kills(), 0);

        app.request_app_tab_close(tab_id);
        app.handle_key(KeyEvent::new(KeyCode::Char('y'), KeyModifiers::NONE));
        assert!(app.ui.app_tabs.tab(tab_id).is_none());
        assert!(app.ui.vault_workspaces.get(tab_id).is_none());
        assert_eq!(probe.kills(), 1);
        assert_eq!(probe.waits(), 1);
    });
}
