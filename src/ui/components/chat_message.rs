use ratatui::{layout::Rect, prelude::*, widgets::*, Frame};

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

        if self.role == "assistant" {
            let mut lines = vec![Line::from(Span::styled(
                "Assistant",
                Style::default().fg(Color::Cyan).bold(),
            ))];
            lines.extend(self.content.lines().map(Line::from));

            let paragraph = Paragraph::new(lines)
                .block(
                    Block::default()
                        .borders(Borders::ALL)
                        .border_style(Style::default().fg(theme.border).bg(theme.panel))
                        .style(Style::default().fg(theme.foreground).bg(theme.panel)),
                )
                .style(Style::default().fg(theme.foreground).bg(theme.panel))
                .wrap(Wrap { trim: true });

            f.render_widget(paragraph, area);
            return;
        }

        if self.role == "user" {
            let paragraph = Paragraph::new(self.content)
                .block(
                    Block::default()
                        .borders(Borders::LEFT)
                        .border_style(Style::default().fg(theme.accent_alt)),
                )
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
