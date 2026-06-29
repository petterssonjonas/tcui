use ratatui::{layout::Rect, prelude::*, widgets::*, Frame};

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
        let content = vec![
            Line::from(Span::raw("Configure MCP Server")),
            Line::from(Span::raw("")),
            Line::from(Span::raw(format!("Name: {}", self.name))),
            Line::from(Span::raw(format!("Transport: {}", self.transport))),
            Line::from(Span::raw("")),
            Line::from(Span::raw("[Enter] Save  [Esc] Cancel")),
        ];

        let paragraph = Paragraph::new(content)
            .block(Block::default().title("MCP Server").borders(Borders::ALL));

        f.render_widget(paragraph, area);
    }
}

impl Default for McpConfigModal {
    fn default() -> Self {
        Self::new()
    }
}
