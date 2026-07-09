use crate::{config::ToastPosition, ui::toast::*};
use ratatui::{backend::TestBackend, layout::Rect, Terminal};

#[test]
fn stack_keeps_only_five_newest_toasts() {
    // Given
    let mut stack = ToastStack::new();

    // When
    for idx in 0..6 {
        stack.push_message(format!("toast {idx}"), idx);
    }
    let mut terminal = Terminal::new(TestBackend::new(80, 24)).expect("test terminal");
    terminal
        .draw(|frame| {
            render(
                frame,
                Rect::new(0, 0, 80, 24),
                &mut stack,
                6,
                ToastPosition::TopLeft,
                0,
            );
        })
        .expect("render toasts");

    // Then
    let screen = terminal.backend().to_string();
    assert_eq!(stack.len(), 5);
    assert!(!screen.contains("toast 0"));
    assert!(screen.contains("toast 1"));
    assert!(screen.contains("toast 5"));
}

#[test]
fn stack_expires_toasts_by_duration() {
    // Given
    let mut stack = ToastStack::new();
    stack.push(Toast::with_level(
        "short".to_string(),
        ToastLevel::Info,
        10,
        5,
    ));

    // When
    stack.expire(15);

    // Then
    assert!(stack.is_empty());
}

#[test]
fn render_stacks_multiple_toasts() {
    // Given
    let mut stack = ToastStack::new();
    stack.push_message("first".to_string(), 0);
    stack.push(Toast::with_level(
        "second".to_string(),
        ToastLevel::Success,
        1,
        180,
    ));
    let mut terminal = Terminal::new(TestBackend::new(80, 24)).expect("test terminal");

    // When
    terminal
        .draw(|frame| {
            render(
                frame,
                Rect::new(0, 0, 80, 24),
                &mut stack,
                2,
                ToastPosition::TopRight,
                0,
            );
        })
        .expect("render toasts");

    // Then
    let screen = terminal.backend().to_string();
    assert!(screen.contains("first"));
    assert!(screen.contains("second"));
    assert!(screen.contains("Success"));
}

#[test]
fn off_position_skips_rendering_without_expiring() {
    // Given
    let mut stack = ToastStack::new();
    stack.push_message("hidden".to_string(), 0);
    let mut terminal = Terminal::new(TestBackend::new(80, 24)).expect("test terminal");

    // When
    terminal
        .draw(|frame| {
            render(
                frame,
                Rect::new(0, 0, 80, 24),
                &mut stack,
                1,
                ToastPosition::Off,
                0,
            );
        })
        .expect("render off toasts");

    // Then
    assert_eq!(stack.len(), 1);
    assert!(!terminal.backend().to_string().contains("hidden"));
}

#[test]
fn right_sidebar_width_offsets_top_right_toast_left() {
    // Given
    let toast = Toast::new("right".to_string(), 0);

    // When
    let rect = toast_rect(
        Rect::new(0, 0, 100, 24),
        &toast,
        ToastPosition::TopRight,
        30,
        2,
    )
    .expect("toast rect");

    // Then
    assert!(rect.right() <= 68);
}

#[test]
fn top_left_and_top_center_positions_use_expected_columns() {
    // Given
    let toast = Toast::new("position".to_string(), 0);

    // When
    let left = toast_rect(
        Rect::new(0, 0, 90, 24),
        &toast,
        ToastPosition::TopLeft,
        0,
        2,
    )
    .expect("left rect");
    let center = toast_rect(
        Rect::new(0, 0, 90, 24),
        &toast,
        ToastPosition::TopCenter,
        0,
        2,
    )
    .expect("center rect");

    // Then
    assert_eq!(left.x, TOAST_MARGIN);
    assert!(center.x > left.x);
    assert!(center.x < 45);
}
