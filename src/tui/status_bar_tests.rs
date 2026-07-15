use crate::tui::status_bar::{ConnectionStatus, StatusBar, StatusBarAreas, StatusBarConfig};
use ratatui::{backend::TestBackend, layout::Rect, Terminal};

fn rendered(status: StatusBar, height: u16) -> (String, StatusBarAreas) {
    let mut terminal = Terminal::new(TestBackend::new(120, 30)).expect("test terminal");
    let mut areas = StatusBarAreas::default();
    terminal
        .draw(|frame| {
            areas = status.render(frame, Rect::new(0, 30 - height, 120, height));
        })
        .expect("render status");
    (terminal.backend().to_string(), areas)
}

fn base_status() -> StatusBar {
    StatusBar {
        status: ConnectionStatus::Failed,
        message: None,
        mcps: vec!["filesystem".to_string(), "github".to_string()],
        working: false,
        tick: 0,
        provider: "OpenAI".to_string(),
        model: "deepseek-v4-flash".to_string(),
        reasoning_effort: Some("medium".to_string()),
        show_reasoning_selector: true,
        show_selector: true,
        web_search_enabled: true,
        context_window: Some(256_000),
        context_used_tokens: Some(128_000),
        config: StatusBarConfig::default(),
    }
}

#[test]
fn default_status_widgets_preserve_click_hit_areas() {
    // Given
    let status = base_status();

    // When
    let (screen, areas) = rendered(status, 1);

    // Then
    assert!(screen.contains("web on"));
    assert!(screen.contains("OpenAI"));
    assert!(screen.contains("deepseek-v4-flash"));
    assert!(screen.contains("medium"));
    assert!(screen.contains("Context: 50% used of 256K"));
    assert!(screen.contains("Not connected, check settings"));
    assert!(areas.web_search.is_some());
    assert!(areas.provider.is_some());
    assert!(areas.model.is_some());
    assert!(areas.reasoning.is_some());
}

#[test]
fn two_row_layout_places_widgets_on_second_row() {
    // Given
    let mut status = base_status();
    status.config = StatusBarConfig {
        rows: 2,
        widgets: vec![
            crate::config::StatusWidgetPlacement {
                id: "provider".to_string(),
                row: 1,
                area: 1,
            },
            crate::config::StatusWidgetPlacement {
                id: "connection".to_string(),
                row: 2,
                area: 6,
            },
        ],
    };

    // When
    let (screen, areas) = rendered(status, 2);

    // Then
    assert!(screen.contains("OpenAI"));
    assert!(screen.contains("Not connected, check settings"));
    assert_eq!(areas.provider.map(|area| area.y), Some(28));
}

#[test]
fn placement_validation_clamps_row_and_area() {
    // Given
    let mut status = base_status();
    status.config = StatusBarConfig {
        rows: 2,
        widgets: vec![crate::config::StatusWidgetPlacement {
            id: "provider".to_string(),
            row: 9,
            area: 9,
        }],
    };

    // When
    let (_, areas) = rendered(status, 2);

    // Then
    assert_eq!(areas.provider.map(|area| (area.y, area.x)), Some((29, 85)));
}

#[test]
fn selector_toggle_hides_provider_and_model_but_not_web_search() {
    // Given
    let mut status = base_status();
    status.show_selector = false;

    // When
    let (screen, areas) = rendered(status, 1);

    // Then
    assert!(screen.contains("web on"));
    assert!(!screen.contains("OpenAI"));
    assert!(!screen.contains("deepseek-v4-flash"));
    assert!(areas.web_search.is_some());
    assert!(areas.provider.is_none());
    assert!(areas.model.is_none());
}

#[test]
fn tools_widget_accepts_mcps_alias() {
    // Given
    let mut status = base_status();
    status.config = StatusBarConfig {
        rows: 1,
        widgets: vec![crate::config::StatusWidgetPlacement {
            id: "mcps".to_string(),
            row: 1,
            area: 1,
        }],
    };

    // When
    let (screen, _) = rendered(status, 1);

    // Then
    assert!(screen.contains("Tools: 2"));
}
