use super::*;

use crate::app::submission::PendingDocumentInput;

mod composer;
mod lifecycle;
mod origins;

fn queue_document(app: &mut TuiApp, source: &str, name: &str, content: &[u8]) {
    let runtime = tokio::runtime::Runtime::new().expect("create runtime");
    runtime
        .block_on(
            app.dispatch(Action::QueuePendingDocument(PendingDocumentInput {
                source_identity: source.to_string(),
                display_name: name.to_string(),
                bytes: content.to_vec(),
            })),
        )
        .expect("queue pending document");
}

fn render_text(app: &mut TuiApp, width: u16, height: u16) -> String {
    let mut terminal = ratatui::Terminal::new(ratatui::backend::TestBackend::new(width, height))
        .expect("test terminal");
    terminal
        .draw(|frame| app.ui.render(frame))
        .expect("render app");
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

fn isolate_chat_surface(app: &mut TuiApp) {
    app.ui.panel_state.left = crate::tui::shell::PanelMode::Closed;
    app.ui.panel_state.right = crate::tui::shell::PanelMode::Closed;
    app.ui.sidebar_open = false;
    app.ui.artifact_sidebar_open = false;
    app.ui.toast_stack = crate::ui::toast::ToastStack::new();
}
