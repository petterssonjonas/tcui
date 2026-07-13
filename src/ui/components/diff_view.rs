use ratatui::{Frame, layout::Rect, prelude::*, widgets::*};

pub struct DiffView<'a> {
    pub path: &'a str,
    pub diff: &'a str,
    pub status: DiffStatus,
}

#[derive(Debug, Clone, Copy)]
pub enum DiffStatus {
    Pending,
    Accepted,
    Rejected,
}

impl<'a> DiffView<'a> {
    pub fn new(path: &'a str, diff: &'a str) -> Self {
        Self {
            path,
            diff,
            status: DiffStatus::Pending,
        }
    }

    pub fn render(&self, f: &mut Frame, area: Rect) {
        let status_text = match self.status {
            DiffStatus::Pending => "[Accept] [Reject]",
            DiffStatus::Accepted => "[Accepted]",
            DiffStatus::Rejected => "[Rejected]",
        };

        let header = format!("File Edit: {} {}", self.path, status_text);
        let lines: Vec<Line> = std::iter::once(Line::from(Span::raw(header)))
            .chain(self.diff.lines().map(|line| {
                let style = if line.starts_with('+') {
                    Style::default().fg(Color::Green)
                } else if line.starts_with('-') {
                    Style::default().fg(Color::Red)
                } else {
                    Style::default()
                };
                Line::from(Span::styled(line, style))
            }))
            .collect();

        let theme = crate::theme::active_theme();
        let paragraph = Paragraph::new(lines)
            .style(Style::default().bg(theme.code_bg))
            .wrap(Wrap { trim: true });

        f.render_widget(paragraph, area);
    }
}
