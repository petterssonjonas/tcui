use super::*;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

#[test]
fn chat_file_sidebar_chat_link_row_has_only_view_and_delete_actions() {
    with_sidebar_app("chat-actions", |app, _| {
        // Given
        let (handle, _) = create_link(app, "chat.md", b"chat contents");
        let chat_files = app.ui.tabs[0].temporary_artifacts.clone();
        let mut state = ArtifactSidebarState::default();

        // When
        let terminal = render_sidebar(&chat_files, &[], ALL_SECTIONS, &mut state, 48, 20);

        // Then
        let rendered = terminal.backend().to_string();
        assert!(rendered.contains("[View]"));
        assert!(rendered.contains("[Del]"));
        assert!(!rendered.contains("[Save]"));
        assert_eq!(
            chat_files[0].origin,
            crate::ui::artifact_sidebar::ArtifactOrigin::Chat
        );
        let view = label_position(terminal.backend().buffer(), "[View]");
        let delete = label_position(terminal.backend().buffer(), "[Del]");
        assert_eq!(
            state.action_at(view),
            Some(ArtifactSidebarAction::Open(handle.clone()))
        );
        assert_eq!(
            state.action_at(delete),
            Some(ArtifactSidebarAction::Delete(handle))
        );
        assert_no_save_action(&state, 48, 20);
    });
}

#[test]
fn chat_file_sidebar_saved_files_are_global_while_chat_links_are_isolated() {
    with_sidebar_app("global-saved", |app, _| {
        // Given
        let first_conversation = app.ui.tabs[0].active_conversation;
        create_link(app, "global.md", b"global contents");
        let first_chat = app.ui.tabs[0].temporary_artifacts.clone();
        let saved = app.ui.saved_artifacts.clone();
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
            &app.ui.saved_artifacts,
            ALL_SECTIONS,
            &mut second_state,
            48,
            24,
        );

        // Then
        assert_eq!(first.backend().to_string().matches("global.md").count(), 2);
        assert_eq!(second.backend().to_string().matches("global.md").count(), 1);
        assert!(second.backend().to_string().contains("Saved files"));
        assert!(second_chat.is_empty());
        app.load_conversation_into_tab(0, first_conversation)
            .expect("reload first chat");
        assert_eq!(app.ui.tabs[0].temporary_artifacts.len(), 1);
    });
}

#[test]
fn chat_file_sidebar_tiny_widths_never_render_or_route_save() {
    with_sidebar_app("tiny-widths", |app, _| {
        // Given
        create_link(app, "tiny.md", b"tiny contents");
        let chat_files = app.ui.tabs[0].temporary_artifacts.clone();

        // When / Then
        for width in 1_u16..=18 {
            let mut state = ArtifactSidebarState::default();
            let terminal = render_sidebar(&chat_files, &[], ALL_SECTIONS, &mut state, width, 12);
            assert!(!terminal.backend().to_string().contains("[Save]"));
            assert_no_save_action(&state, width, 12);
            for y in 0..12 {
                assert!(state.action_at(Position::new(width, y)).is_none());
            }
        }
    });
}

#[test]
fn chat_file_sidebar_hidden_sections_reject_stale_geometry() {
    let chat_files = vec![ArtifactEntry::temp_markdown(
        1,
        "chat.md".to_string(),
        "chat contents".to_string(),
    )];
    let saved_files = vec![ArtifactEntry::saved_file(PathBuf::from("saved.md"))];
    let mut state = ArtifactSidebarState::default();
    let visible = render_sidebar(&chat_files, &saved_files, ALL_SECTIONS, &mut state, 48, 24);
    let stale_saved = label_position(visible.backend().buffer(), "saved.md");

    render_sidebar(
        &chat_files,
        &saved_files,
        ArtifactSidebarVisibility {
            saved_files: false,
            memories: false,
            markdown_vault: false,
        },
        &mut state,
        48,
        24,
    );

    assert!(state.saved_header.is_none());
    assert!(state.saved_body.is_none());
    assert!(state.action_at(stale_saved).is_none());
}

#[test]
fn chat_file_sidebar_labels_follow_optional_section_visibility() {
    let chat_files = vec![ArtifactEntry::temp_markdown(
        1,
        "chat.md".to_string(),
        "chat contents".to_string(),
    )];

    for mask in 0_u8..8 {
        let visibility = ArtifactSidebarVisibility {
            saved_files: mask & 0b001 != 0,
            memories: mask & 0b010 != 0,
            markdown_vault: mask & 0b100 != 0,
        };
        let mut state = ArtifactSidebarState::default();
        let terminal = render_sidebar(&chat_files, &[], visibility, &mut state, 48, 32);
        let rendered = terminal.backend().to_string();

        assert!(rendered.contains("Chat files"), "mask {mask:03b}");
        assert_eq!(rendered.contains("Saved files"), visibility.saved_files);
        assert_eq!(rendered.contains("Memories"), visibility.memories);
        assert_eq!(
            rendered.contains("Markdown Vault"),
            visibility.markdown_vault
        );
    }
}

#[test]
fn chat_file_sidebar_visibility_settings_persist_across_reload() {
    with_test_app_config("chat-file-sidebar-settings", AppConfig::default(), |app| {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");
        for id in ["show_saved_files", "show_memories", "show_markdown_vault"] {
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

        assert!(!app.ui.show_saved_files);
        assert!(!app.ui.show_memories);
        assert!(!app.ui.show_markdown_vault);
        let reloaded = AppConfig::load().expect("reload persisted visibility settings");
        assert!(!reloaded.show_saved_files);
        assert!(!reloaded.show_memories);
        assert!(!reloaded.show_markdown_vault);
    });
}

#[test]
fn chat_file_sidebar_wide_overlay_omits_hidden_sections() {
    let config = AppConfig {
        show_saved_files: false,
        show_memories: false,
        show_markdown_vault: false,
        ..AppConfig::default()
    };
    with_test_app_config("chat-file-sidebar-wide", config, |app| {
        app.ui.panel_state.right = crate::tui::shell::PanelMode::Wide;
        app.ui.artifact_sidebar_open = true;

        let rendered = render_app(app, 100, 30);

        assert!(rendered.contains("Chat files"));
        assert!(!rendered.contains("Saved files"));
        assert!(!rendered.contains("Memories"));
        assert!(!rendered.contains("Markdown Vault"));
        assert!(app.ui.artifact_sidebar_state.temp_header.is_some());
        assert!(app.ui.artifact_sidebar_state.saved_header.is_none());
        assert!(app.ui.artifact_sidebar_state.memory_header.is_none());
        assert!(app.ui.artifact_sidebar_state.vault_header.is_none());
    });
}
