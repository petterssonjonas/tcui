use ratatui::{Frame, layout::Rect, prelude::*, widgets::*};
use std::env;

fn user_display_name() -> String {
    if let Ok(user) = env::var("USER") {
        if !user.is_empty() {
            return user;
        }
    }
    if let Ok(user) = env::var("USERNAME") {
        if !user.is_empty() {
            return user;
        }
    }
    "User".to_string()
}

pub struct ChatMessage<'a> {
    pub role: &'a str,
    pub content: &'a str,
    pub thinking: Option<&'a str>,
    pub collapsed: bool,
}

impl<'a> ChatMessage<'a> {
    pub fn new(role: &'a str, content: &'a str) -> Self {
        Self {
            role,
            content,
            thinking: None,
            collapsed: true,
        }
    }

    pub fn render(&self, f: &mut Frame, area: Rect) {
        let theme = crate::theme::active_theme();

        if self.role == "user" {
            let username = user_display_name();
            let mut lines = vec![Line::from(Span::styled(
                username,
                Style::default().fg(Color::Cyan).bold(),
            ))];
            lines.extend(self.content.lines().map(Line::from));

            let paragraph = Paragraph::new(lines)
                .style(Style::default().fg(theme.foreground).bg(theme.user_bubble))
                .wrap(Wrap { trim: true });

            f.render_widget(paragraph, area);
            return;
        }

        if self.role == "assistant" {
            let paragraph = Paragraph::new(self.content)
                .style(Style::default().fg(theme.foreground))
                .wrap(Wrap { trim: true });

            f.render_widget(paragraph, area);
            return;
        }

        let paragraph = Paragraph::new(self.content)
            .style(Style::default().fg(theme.foreground))
            .wrap(Wrap { trim: true });

        f.render_widget(paragraph, area);
    }
}
