#![allow(dead_code)]
use ratatui::{layout::Rect, prelude::*, widgets::*, Frame};

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
        let items: Vec<ListItem> = self
            .conversations
            .iter()
            .map(|c| ListItem::new(c.title.clone()))
            .collect();

        let list = List::new(items)
            .block(
                Block::default()
                    .title("Sessions [Ctrl+S]")
                    .borders(Borders::ALL),
            )
            .highlight_style(Style::default().bg(Color::DarkGray));

        f.render_widget(list, area);
    }
}
