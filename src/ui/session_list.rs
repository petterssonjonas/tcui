#![allow(dead_code)]
use ratatui::{Frame, layout::Rect, prelude::*, widgets::*};

pub struct SessionList<'a> {
    pub conversations: &'a [crate::ui::ConversationEntry],
    pub active_id: i64,
}

impl<'a> SessionList<'a> {
    pub fn new(conversations: &'a [crate::ui::ConversationEntry], active_id: i64) -> Self {
        Self {
            conversations,
            active_id,
        }
    }

    pub fn render(&self, f: &mut Frame, area: Rect) {
        let theme = crate::theme::active_theme();
        let mut items: Vec<ListItem> = vec![ListItem::new(Line::styled(
            "Sessions [Ctrl+S]",
            Style::default()
                .fg(theme.accent)
                .bg(theme.panel)
                .add_modifier(Modifier::BOLD),
        ))];
        items.extend(
            self.conversations
                .iter()
                .map(|c| ListItem::new(c.title.clone())),
        );

        let list = List::new(items)
            .block(Block::default().style(Style::default().bg(theme.panel)))
            .highlight_style(Style::default().bg(Color::DarkGray));

        f.render_widget(list, area);
    }
}
