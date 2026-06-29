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

        let paragraph = Paragraph::new(lines)
            .block(Block::default().title("Confirm Diff").borders(Borders::ALL))
            .wrap(Wrap { trim: true });

        f.render_widget(paragraph, area);
    }
}
