use ratatui::{layout::Rect, prelude::*, widgets::*, Frame};

pub struct NewTabModal {
    pub name: String,
    pub provider: String,
    pub model: String,
    pub endpoint: Option<String>,
    pub api_key: Option<String>,
    pub soul: Option<String>,
}

impl NewTabModal {
    pub fn new() -> Self {
        Self {
            name: "New Chat".to_string(),
            provider: "openai".to_string(),
            model: "gpt-4o".to_string(),
            endpoint: None,
            api_key: None,
            soul: None,
        }
    }

    pub fn render(&self, f: &mut Frame, area: Rect) {
        let theme = crate::theme::active_theme();
        let content = vec![
            Line::from(Span::raw("Create New Chat Tab")),
            Line::from(Span::raw("")),
            Line::from(Span::raw(format!("Name: {}", self.name))),
            Line::from(Span::raw(format!("Provider: {}", self.provider))),
            Line::from(Span::raw(format!("Model: {}", self.model))),
            Line::from(Span::raw("")),
            Line::from(Span::raw("[Enter] Save  [Esc] Cancel")),
        ];

        let block = Block::default().style(Style::default().bg(theme.panel));
        f.render_widget(block, area);
        f.render_widget(
            Paragraph::new(Line::from(" New Tab ")).style(
                Style::default()
                    .fg(theme.accent)
                    .add_modifier(Modifier::BOLD),
            ),
            Rect::new(area.x, area.y, area.width, 1),
        );

        let content_area = Rect::new(
            area.x,
            area.y + 1,
            area.width,
            area.height.saturating_sub(1),
        );
        let paragraph = Paragraph::new(content).alignment(Alignment::Left);
        f.render_widget(paragraph, content_area);
    }
}

impl Default for NewTabModal {
    fn default() -> Self {
        Self::new()
    }
}
