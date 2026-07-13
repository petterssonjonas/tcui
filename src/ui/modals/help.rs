use ratatui::{Frame, layout::Rect, prelude::*, widgets::*};

pub struct HelpModal;

impl HelpModal {
    pub fn new() -> Self {
        Self
    }

    pub fn render(&self, f: &mut Frame, area: Rect) {
        let theme = crate::theme::active_theme();
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

        let block = Block::default().style(Style::default().bg(theme.panel));
        f.render_widget(block, area);
        f.render_widget(
            Paragraph::new(Line::from(" Keyboard Shortcuts ")).style(
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
        let paragraph = Paragraph::new(lines).wrap(Wrap { trim: true });
        f.render_widget(paragraph, content_area);
    }
}

impl Default for HelpModal {
    fn default() -> Self {
        Self::new()
    }
}
