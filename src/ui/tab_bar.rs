#![allow(dead_code)]
use ratatui::{layout::Rect, prelude::*, widgets::*, Frame};

pub struct TabBar<'a> {
    pub tabs: &'a [crate::ui::ChatTabState],
    pub active: usize,
}

impl<'a> TabBar<'a> {
    pub fn new(tabs: &'a [crate::ui::ChatTabState], active: usize) -> Self {
        Self { tabs, active }
    }

    pub fn render(&self, f: &mut Frame, area: Rect) {
        let theme = crate::theme::active_theme();
        let mut items: Vec<Line> = vec![Line::styled(
            "Tabs",
            Style::default()
                .fg(theme.accent)
                .bg(theme.panel)
                .add_modifier(Modifier::BOLD),
        )];
        items.extend(self.tabs.iter().enumerate().map(|(i, t)| {
            let prefix = if i == self.active { ">" } else { " " };
            Line::raw(format!("{} {}", prefix, t.tab.name))
        }));

        let tab_bar =
            Paragraph::new(items).block(Block::default().style(Style::default().bg(theme.panel)));

        f.render_widget(tab_bar, area);
    }
}
