use super::*;

#[test]
fn chat_file_sidebar_collapse_clears_row_hit_areas_immediately() {
    with_sidebar_app("collapse-geometry", |app, _| {
        // Given
        create_link(app, "collapse.md", b"contents");
        let chat_files = app.ui.tabs[0].temporary_artifacts.clone();
        let mut state = ArtifactSidebarState::default();
        let terminal = render_sidebar(&chat_files, &[], ALL_SECTIONS, &mut state, 48, 20);
        let old_view = label_position(terminal.backend().buffer(), "[View]");

        // When
        state.toggle(crate::ui::artifact_sidebar::ArtifactSection::Temporary);

        // Then
        assert!(state.action_at(old_view).is_none());
    });
}

#[test]
fn chat_file_sidebar_scroll_clears_then_rebuilds_visible_row_actions() {
    with_sidebar_app("scroll-geometry", |app, _| {
        // Given
        for index in 1..=4 {
            let name = format!("file-{index}.md");
            create_link(app, &name, name.as_bytes());
        }
        let chat_files = app.ui.tabs[0].temporary_artifacts.clone();
        let second_handle = chat_files[1].handle.clone();
        let mut state = ArtifactSidebarState::default();
        let first = render_sidebar(&chat_files, &[], ALL_SECTIONS, &mut state, 48, 24);
        let top_view = label_position(first.backend().buffer(), "[View]");

        // When
        state.scroll(
            crate::ui::artifact_sidebar::ArtifactSection::Temporary,
            true,
            chat_files.len(),
            3,
        );

        // Then
        assert!(state.action_at(top_view).is_none());
        let _ = render_sidebar(&chat_files, &[], ALL_SECTIONS, &mut state, 48, 24);
        assert_eq!(
            state.action_at(top_view),
            Some(ArtifactSidebarAction::Open(second_handle))
        );
    });
}

#[test]
fn chat_file_sidebar_conversation_switch_clears_old_row_geometry() {
    with_sidebar_app("switch-geometry", |app, _| {
        // Given
        create_link(app, "first.md", b"first");
        let chat_files = app.ui.tabs[0].temporary_artifacts.clone();
        let mut state = ArtifactSidebarState::default();
        let terminal = render_sidebar(&chat_files, &[], ALL_SECTIONS, &mut state, 48, 20);
        let old_view = label_position(terminal.backend().buffer(), "[View]");
        app.ui.artifact_sidebar_state = state;
        let second_conversation = app
            .storage
            .create_conversation(0)
            .expect("create second chat");

        // When
        app.load_conversation_into_tab(0, second_conversation)
            .expect("switch conversation");

        // Then
        assert!(app.ui.artifact_sidebar_state.action_at(old_view).is_none());
    });
}

#[test]
fn chat_file_sidebar_refresh_clears_old_row_geometry() {
    with_sidebar_app("refresh-geometry", |app, _| {
        // Given
        create_link(app, "refresh.md", b"contents");
        let chat_files = app.ui.tabs[0].temporary_artifacts.clone();
        let mut state = ArtifactSidebarState::default();
        let terminal = render_sidebar(&chat_files, &[], ALL_SECTIONS, &mut state, 48, 20);
        let old_view = label_position(terminal.backend().buffer(), "[View]");
        app.ui.artifact_sidebar_state = state;

        // When
        let conversation_id = app.ui.tabs[0].active_conversation;
        app.refresh_canonical_file_catalogs(0, conversation_id)
            .expect("refresh catalogs");

        // Then
        assert!(app.ui.artifact_sidebar_state.action_at(old_view).is_none());
    });
}

#[test]
fn chat_file_sidebar_resize_rejects_old_offscreen_geometry() {
    with_sidebar_app("resize-geometry", |app, _| {
        // Given
        create_link(app, "resize.md", b"contents");
        let chat_files = app.ui.tabs[0].temporary_artifacts.clone();
        let mut state = ArtifactSidebarState::default();
        let wide = render_sidebar(&chat_files, &[], ALL_SECTIONS, &mut state, 48, 20);
        let old_delete = label_position(wide.backend().buffer(), "[Del]");

        // When
        let narrow = render_sidebar(&chat_files, &[], ALL_SECTIONS, &mut state, 8, 12);

        // Then
        assert!(!narrow.backend().to_string().contains("[Save]"));
        assert!(state.action_at(old_delete).is_none());
    });
}
