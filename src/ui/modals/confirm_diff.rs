use ratatui::{layout::Rect, prelude::*, widgets::*, Frame};

pub struct ConfirmDiffModal<'a> {
    pub path: &'a str,
    pub diff: &'a str,
}

impl<'a> ConfirmDiffModal<'a> {
    pub fn new(path: &'a str, diff: &'a str) -> Self {
        Self { path, diff }
    }

    pub fn render(&self, f: &mut Frame, area: Rect) {
        let theme = crate::theme::active_theme();
        let header = Line::from(Span::raw(format!("Confirm File Edit: {}", self.path)));
        let diff_lines: Vec<Line> = self
            .diff
            .lines()
            .map(|line| {
                let style = if line.starts_with('+') {
                    Style::default().fg(Color::Green)
                } else if line.starts_with('-') {
                    Style::default().fg(Color::Red)
                } else {
                    Style::default()
                };
                Line::from(Span::styled(line, style))
            })
            .collect();
        let footer = Line::from(Span::raw("[Y] Accept  [N] Reject  [Esc] Cancel"));

        let mut lines = vec![header];
        lines.extend(diff_lines);
        lines.push(footer);

        let block = Block::default().style(Style::default().bg(theme.panel));
        f.render_widget(block, area);
        f.render_widget(
            Paragraph::new(Line::from(" Confirm Diff ")).style(
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
