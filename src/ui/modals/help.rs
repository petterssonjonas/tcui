use ratatui::{layout::Rect, prelude::*, widgets::*, Frame};

pub struct HelpModal;

impl HelpModal {
    pub fn new() -> Self {
        Self
    }

    pub fn render(&self, f: &mut Frame, area: Rect) {
        let keys = vec![
            ("Ctrl+Tab", "Next tab"),
            ("Ctrl+Shift+Tab", "Previous tab"),
            ("Ctrl+N", "New chat tab"),
            ("Ctrl+W", "Close tab"),
            ("Ctrl+S", "Toggle session list"),
            ("Ctrl+L", "Focus input"),
            ("Ctrl+C", "Cancel streaming"),
            ("Ctrl+Q", "Quit"),
            ("Enter", "Send message"),
            ("Esc", "Cancel/blur"),
        ];

        let lines: Vec<Line> = keys
            .iter()
            .map(|&(key, desc)| Line::from(vec![Span::raw(format!("{:20}", key)), Span::raw(desc)]))
            .collect();

        let paragraph = Paragraph::new(lines)
            .block(
                Block::default()
                    .title("Keyboard Shortcuts")
                    .borders(Borders::ALL),
            )
            .wrap(Wrap { trim: true });

        f.render_widget(paragraph, area);
    }
}

impl Default for HelpModal {
    fn default() -> Self {
        Self::new()
    }
}
