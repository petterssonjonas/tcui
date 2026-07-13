use ratatui::{Frame, layout::Rect, prelude::*, widgets::*};

pub struct McpConfigModal {
    pub name: String,
    pub transport: String,
    pub command: Option<String>,
    pub url: Option<String>,
}

impl McpConfigModal {
    pub fn new() -> Self {
        Self {
            name: String::new(),
            transport: "stdio".to_string(),
            command: None,
            url: None,
        }
    }

    pub fn render(&self, f: &mut Frame, area: Rect) {
        let theme = crate::theme::active_theme();
        let content = vec![
            Line::from(Span::raw("Configure MCP Server")),
            Line::from(Span::raw("")),
            Line::from(Span::raw(format!("Name: {}", self.name))),
            Line::from(Span::raw(format!("Transport: {}", self.transport))),
            Line::from(Span::raw("")),
            Line::from(Span::raw("[Enter] Save  [Esc] Cancel")),
        ];

        let block = Block::default().style(Style::default().bg(theme.panel));
        f.render_widget(block, area);
        f.render_widget(
            Paragraph::new(Line::from(" MCP Server ")).style(
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
        let paragraph = Paragraph::new(content);
        f.render_widget(paragraph, content_area);
    }
}

impl Default for McpConfigModal {
    fn default() -> Self {
        Self::new()
    }
}
