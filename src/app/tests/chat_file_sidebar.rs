use super::with_test_app_config;
use crate::config::AppConfig;
use crate::tui::settings_panel::{all_settings, SettingCategory, SettingsPanelState};
use crate::ui::artifact_sidebar::{
    ArtifactEntry, ArtifactSidebar, ArtifactSidebarAction, ArtifactSidebarState,
    ArtifactSidebarVisibility,
};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::{backend::TestBackend, layout::Position, layout::Rect, Terminal};

fn render_sidebar(
    visibility: ArtifactSidebarVisibility,
    state: &mut ArtifactSidebarState,
    width: u16,
    height: u16,
) -> String {
    let chat_files = vec![ArtifactEntry::temp_markdown(
        1,
        "chat.md".to_string(),
        "chat contents".to_string(),
    )];
    let mut terminal = Terminal::new(TestBackend::new(width, height)).expect("test terminal");
    {
        let mut sidebar = ArtifactSidebar::new(&chat_files, &[], &[], &[], false, state)
            .with_visibility(visibility);
        terminal
            .draw(|frame| sidebar.render(frame, Rect::new(0, 0, width, height)))
            .expect("render artifact sidebar");
    }
    terminal.backend().to_string()
}

#[test]
fn labels_follow_every_optional_section_visibility_combination() {
    for mask in 0_u8..8 {
        let visibility = ArtifactSidebarVisibility {
            saved_files: mask & 0b001 != 0,
            memories: mask & 0b010 != 0,
            markdown_vault: mask & 0b100 != 0,
        };
        let mut state = ArtifactSidebarState::default();

        let rendered = render_sidebar(visibility, &mut state, 48, 32);

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
fn hiding_optional_sections_rejects_their_stale_click_geometry() {
    let mut state = ArtifactSidebarState::default();
    let all_visible = ArtifactSidebarVisibility {
        saved_files: true,
        memories: true,
        markdown_vault: true,
    };
    render_sidebar(all_visible, &mut state, 48, 32);
    let stale_positions = [
        (
            state.saved_header.expect("saved header"),
            ArtifactSidebarAction::ToggleSection(
                crate::ui::artifact_sidebar::ArtifactSection::Saved,
            ),
        ),
        (
            state.memory_header.expect("memory header"),
            ArtifactSidebarAction::ToggleSection(
                crate::ui::artifact_sidebar::ArtifactSection::Memories,
            ),
        ),
        (
            state.vault_header.expect("vault header"),
            ArtifactSidebarAction::ToggleSection(
                crate::ui::artifact_sidebar::ArtifactSection::Vault,
            ),
        ),
    ];

    render_sidebar(
        ArtifactSidebarVisibility {
            saved_files: false,
            memories: false,
            markdown_vault: false,
        },
        &mut state,
        48,
        32,
    );

    assert!(state.temp_header.is_some());
    assert!(state.saved_header.is_none());
    assert!(state.saved_body.is_none());
    assert!(state.memory_header.is_none());
    assert!(state.memory_body.is_none());
    assert!(state.vault_header.is_none());
    assert!(state.vault_body.is_none());
    for (area, stale_action) in stale_positions {
        assert_ne!(
            state.action_at(Position::new(area.x, area.y)),
            Some(stale_action)
        );
    }
}

#[test]
fn tiny_sidebar_widths_render_without_out_of_bounds_hit_areas() {
    for width in 1_u16..=8 {
        let mut state = ArtifactSidebarState::default();

        let rendered = render_sidebar(
            ArtifactSidebarVisibility {
                saved_files: true,
                memories: true,
                markdown_vault: true,
            },
            &mut state,
            width,
            12,
        );

        assert!(!rendered.is_empty());
        for area in [
            state.temp_header,
            state.saved_header,
            state.memory_header,
            state.vault_header,
            state.temp_body,
            state.saved_body,
            state.memory_body,
            state.vault_body,
        ]
        .into_iter()
        .flatten()
        {
            assert!(area.right() <= width, "width {width}, area {area:?}");
        }
        for y in 0..12 {
            for x in width..width.saturating_add(16) {
                assert!(
                    state.action_at(Position::new(x, y)).is_none(),
                    "width {width}, out-of-bounds hit at ({x}, {y})"
                );
            }
        }
    }
}

#[test]
fn settings_visibility_toggles_persist_across_reload() {
    with_test_app_config("chat-file-sidebar-settings", AppConfig::default(), |app| {
        for id in ["show_saved_files", "show_memories", "show_markdown_vault"] {
            let catalog = all_settings();
            let mut panel = SettingsPanelState::new().with_sidebar_visibility(
                app.ui.show_saved_files,
                app.ui.show_memories,
                app.ui.show_markdown_vault,
            );
            panel.depth_stack.push(SettingCategory::Obsidian);
            let index = panel
                .results(&catalog)
                .iter()
                .position(|(_, setting)| setting.id == id)
                .expect("sidebar visibility setting");
            panel.select(index, &catalog);
            app.ui.settings_v2 = Some(panel);

            let action = app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));

            assert!(action.is_none());
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
fn wide_overlay_omits_hidden_optional_sections() {
    let config = AppConfig {
        show_saved_files: false,
        show_memories: false,
        show_markdown_vault: false,
        ..AppConfig::default()
    };
    with_test_app_config("chat-file-sidebar-wide", config, |app| {
        app.ui.panel_state.right = crate::tui::shell::PanelMode::Wide;
        app.ui.artifact_sidebar_open = true;
        let mut terminal = Terminal::new(TestBackend::new(100, 30)).expect("test terminal");

        terminal
            .draw(|frame| app.ui.render(frame))
            .expect("render wide Chat files sidebar");

        let rendered = terminal.backend().to_string();
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
