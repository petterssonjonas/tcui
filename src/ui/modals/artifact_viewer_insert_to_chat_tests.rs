use super::{ArtifactViewerProps, ArtifactViewerState};
use crate::config::app_config::{HeadingDownscale, MarkdownMode};
use crate::ui::artifact_sidebar::{ArtifactEntry, ArtifactHandle, ArtifactKind, ArtifactOrigin};
use crate::ui::components::terminal_capabilities::{TerminalCapabilities, TerminalKind};
use ratatui::{backend::TestBackend, Terminal};

fn props() -> ArtifactViewerProps<'static> {
    ArtifactViewerProps {
        markdown_mode: MarkdownMode::Full,
        kitty_enhanced_text: false,
        kitty_heading_downscale: HeadingDownscale::None,
        image_protocol: "off",
        terminal_capabilities: TerminalCapabilities {
            terminal: TerminalKind::Unknown,
            multiplexer: None,
            kitty_graphics: false,
            kitty_text_sizing: false,
            tmux_passthrough: false,
        },
    }
}

fn render(viewer: &mut ArtifactViewerState) -> String {
    let mut terminal = Terminal::new(TestBackend::new(80, 24)).expect("test terminal");
    terminal
        .draw(|frame| viewer.render(frame, frame.area(), props()))
        .expect("render viewer");
    let buffer = terminal.backend().buffer();
    (0..buffer.area.height)
        .map(|y| {
            (0..buffer.area.width).fold(String::new(), |mut row, x| {
                row.push_str(buffer[(x, y)].symbol());
                row
            })
        })
        .collect::<Vec<_>>()
        .join("\n")
}

#[test]
fn insert_to_chat_viewer_button_is_bottom_left_for_eligible_markdown() {
    let _guard = crate::test_support::env_lock().lock().expect("env lock");
    // Given
    let artifact =
        ArtifactEntry::temp_markdown(1, "eligible.md".to_string(), "first\nsecond".to_string());
    let mut viewer = ArtifactViewerState::new(artifact);

    // When
    let screenshot = render(&mut viewer);

    // Then
    let insert = viewer.hit_areas.insert_to_chat.expect("insert hit area");
    let delete = viewer.hit_areas.delete.expect("delete hit area");
    assert!(screenshot.contains("[Insert to chat]"));
    assert!(insert.x < delete.x);
    assert_eq!(insert.y, delete.y);
    println!("INSERT_TO_CHAT_VIEWER_GEOMETRY\n{screenshot}");
}

#[test]
fn insert_to_chat_viewer_button_is_hidden_for_ineligible_documents() {
    let _guard = crate::test_support::env_lock().lock().expect("env lock");
    // Given
    let oversized = "x".repeat(crate::app::submission::MAX_DOCUMENT_BYTES + 1);
    let artifacts = [
        ArtifactEntry {
            handle: ArtifactHandle::Temporary(2),
            name: "binary.bin".to_string(),
            kind: ArtifactKind::Binary,
            origin: ArtifactOrigin::Temporary,
            content: Some("text-shaped but binary-origin".to_string()),
            path: None,
        },
        ArtifactEntry::temp_markdown(3, "empty.md".to_string(), String::new()),
        ArtifactEntry::temp_markdown(4, "oversized.md".to_string(), oversized),
    ];

    // When / Then
    for artifact in artifacts {
        let mut viewer = ArtifactViewerState::new(artifact);
        let screenshot = render(&mut viewer);
        assert!(viewer.hit_areas.insert_to_chat.is_none());
        assert!(!screenshot.contains("[Insert to chat]"));
    }
}

#[test]
fn insert_to_chat_viewer_button_is_hidden_for_view_only_help() {
    let _guard = crate::test_support::env_lock().lock().expect("env lock");
    // Given
    let artifact = ArtifactEntry::temp_markdown(5, "Help".to_string(), "help".to_string());
    let mut viewer = ArtifactViewerState::new(artifact);
    viewer.view_only = true;

    // When
    let screenshot = render(&mut viewer);

    // Then
    assert!(viewer.hit_areas.insert_to_chat.is_none());
    assert!(!screenshot.contains("[Insert to chat]"));
}
