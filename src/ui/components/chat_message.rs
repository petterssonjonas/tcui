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
        let role_indicator = match self.role {
            "user" => "👤 You",
            "assistant" => "🤖 Assistant",
            _ => "•",
        };

        let mut spans = vec![
            Span::styled(role_indicator, Style::default().fg(Color::Cyan).bold()),
            Span::raw(": "),
        ];

        for line in self.content.lines() {
            spans.push(Span::raw(line));
            spans.push(Span::raw("\n"));
        }

        let paragraph = Paragraph::new(Line::from(spans))
            .block(Block::default().borders(Borders::ALL))
            .wrap(Wrap { trim: true });

        f.render_widget(paragraph, area);
    }
}
