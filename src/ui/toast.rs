use ratatui::{
    prelude::*,
    widgets::{Block, Borders, Clear, Paragraph},
    Frame,
};

#[derive(Debug, Clone)]
pub struct Toast {
    pub message: String,
    pub expires_at: u64,
}

impl Toast {
    pub fn new(message: String, frame_tick: u64) -> Self {
        Self {
            message,
            expires_at: frame_tick.saturating_add(180),
        }
    }
}

pub fn render(f: &mut Frame, area: Rect, toast: &mut Option<Toast>, frame_tick: u64) {
    let expired = toast
        .as_ref()
        .is_some_and(|current| frame_tick >= current.expires_at);
    if expired {
        *toast = None;
        return;
    }
    let Some(current) = toast.as_ref() else {
        return;
    };
    if area.width < 24 || area.height < 6 {
        return;
    }

    let theme = crate::theme::active_theme();
    let max_message_width = area.width.saturating_sub(8).clamp(16, 68) as usize;
    let message = trim_message(&current.message, max_message_width);
    let width = (unicode_width::UnicodeWidthStr::width(message.as_str()) + 4)
        .min(area.width.saturating_sub(2) as usize) as u16;
    let toast_area = Rect::new(
        area.right().saturating_sub(width + 1),
        area.y.saturating_add(2),
        width,
        3,
    );
    f.render_widget(Clear, toast_area);
    f.render_widget(
        Block::default()
            .title(" Notice ")
            .borders(Borders::ALL)
            .border_style(Style::default().fg(theme.accent))
            .style(Style::default().bg(theme.panel)),
        toast_area,
    );
    let inner = Rect::new(
        toast_area.x + 1,
        toast_area.y + 1,
        toast_area.width.saturating_sub(2),
        1,
    );
    f.render_widget(
        Paragraph::new(message).style(Style::default().fg(theme.foreground)),
        inner,
    );
}

fn trim_message(message: &str, max_width: usize) -> String {
    if unicode_width::UnicodeWidthStr::width(message) <= max_width {
        return message.to_string();
    }
    let mut trimmed = message
        .chars()
        .take(max_width.saturating_sub(1))
        .collect::<String>();
    trimmed.push('~');
    trimmed
}
