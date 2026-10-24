use super::*;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

#[test]
fn chat_file_sidebar_saved_files_are_global_while_chat_links_are_isolated() {
    with_sidebar_app("global-saved", |app, _| {
        // Given
        let first_conversation = app.ui.tabs[0].active_conversation;
        create_link(app, "global.md", b"global contents");
        let first_chat = app.ui.tabs[0].temporary_artifacts.clone();
        let saved = app.ui.vault_artifacts.clone();
        let mut first_state = ArtifactSidebarState::default();

        // When
        let first = render_sidebar(&first_chat, &saved, ALL_SECTIONS, &mut first_state, 48, 24);
        let second_conversation = app
            .storage
            .create_conversation(0)
            .expect("create second chat");
        app.load_conversation_into_tab(0, second_conversation)
            .expect("load second chat");
        let second_chat = app.ui.tabs[0].temporary_artifacts.clone();
        let mut second_state = ArtifactSidebarState::default();
        let second = render_sidebar(
            &second_chat,
            &app.ui.vault_artifacts,
            ALL_SECTIONS,
            &mut second_state,
            48,
            24,
        );

        // Then
        assert_eq!(first.backend().to_string().matches("global.md").count(), 2);
        assert_eq!(second.backend().to_string().matches("global.md").count(), 1);
        assert!(second_chat.is_empty());
        app.load_conversation_into_tab(0, first_conversation)
            .expect("reload first chat");
        assert_eq!(app.ui.tabs[0].temporary_artifacts.len(), 1);
    });
}

#[test]
fn chat_file_sidebar_visibility_settings_persist_across_reload() {
    with_test_app_config("chat-file-sidebar-settings", AppConfig::default(), |app| {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");
        for id in ["show_memories", "show_vault"] {
            app.open_settings_page(Some(id));
            let action =
                app.settings_workspace_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
            let Some(crate::app::Action::ApplySetting {
                id: changed_id,
                value,
            }) = action
            else {
                panic!("dedicated settings should apply the selected boolean");
            };
            assert_eq!(changed_id, id);
            runtime
                .block_on(app.apply_settings_value(&changed_id, &value))
                .expect("apply visibility");
        }

        assert!(!app.ui.show_memories);
        assert!(!app.ui.show_vault);
        let reloaded = AppConfig::load().expect("reload persisted visibility settings");
        assert!(!reloaded.show_memories);
        assert!(!reloaded.show_vault);
    });
}

#[test]
fn chat_file_sidebar_wide_overlay_omits_hidden_sections() {
    let config = AppConfig {
        show_memories: false,
        show_vault: false,
        ..AppConfig::default()
    };
    with_test_app_config("chat-file-sidebar-wide", config, |app| {
        app.ui.panel_state.right = crate::tui::shell::PanelMode::Wide;
        app.ui.artifact_sidebar_open = true;

        render_app(app, 100, 30);
        assert!(app.ui.artifact_sidebar_state.temp_header.is_some());
        assert!(app.ui.artifact_sidebar_state.memory_header.is_none());
        assert!(app.ui.artifact_sidebar_state.vault_header.is_none());
    });
}
