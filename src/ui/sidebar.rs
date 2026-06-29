#![allow(dead_code)]
use ratatui::{prelude::*, widgets::*, Frame};

pub struct Sidebar<'a> {
    pub conversations: &'a [crate::ui::ConversationEntry],
    pub active_conversation: i64,
    pub show_new_chat_card: bool,
    pub new_chat_card_active: bool,
}

impl<'a> Sidebar<'a> {
    pub fn new(
        conversations: &'a [crate::ui::ConversationEntry],
        active_conversation: i64,
        show_new_chat_card: bool,
        new_chat_card_active: bool,
    ) -> Self {
        Self {
            conversations,
            active_conversation,
            show_new_chat_card,
            new_chat_card_active,
        }
    }

    pub fn render(&self, f: &mut Frame, area: Rect) {
        let theme = crate::theme::active_theme();
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(3), // Title
                Constraint::Min(0),    // Chat list
                Constraint::Length(3), // + New Chat
                Constraint::Length(3), // Settings
            ])
            .split(area);

        let title = Paragraph::new(Line::from(vec![
            Span::styled(
                " TermChat",
                Style::default()
                    .fg(theme.accent)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                format!("  {}", self.conversations.len()),
                Style::default().fg(theme.muted),
            ),
        ]))
        .alignment(Alignment::Left)
        .block(
            Block::default()
                .borders(Borders::BOTTOM)
                .border_style(Style::default().fg(theme.border)),
        );
        f.render_widget(title, chunks[0]);

        // Chat list including "New Chat..." card
        let mut items: Vec<ListItem> = Vec::new();

        // "New Chat..." card for current unstarted conversation
        if self.show_new_chat_card {
            let style = if self.new_chat_card_active {
                theme.selected_style()
            } else {
                Style::default().fg(theme.muted).bg(theme.sidebar)
            };
            let icon = if self.new_chat_card_active {
                "> "
            } else {
                "  "
            };
            items.push(ListItem::new(Line::from(vec![
                Span::styled(icon, style),
                Span::styled("New chat", style.add_modifier(Modifier::ITALIC)),
            ])));
        }

        // Existing conversations
        for c in self.conversations.iter() {
            let is_active = c.id == self.active_conversation;
            let style = if is_active {
                theme.selected_style()
            } else {
                Style::default().fg(theme.foreground).bg(theme.sidebar)
            };
            let icon = if is_active { "> " } else { "  " };
            items.push(ListItem::new(Line::from(vec![
                Span::styled(icon, style),
                Span::styled(c.title.clone(), style),
            ])));
        }

        let list = List::new(items)
            .block(Block::default().borders(Borders::NONE))
            .highlight_style(Style::default().bg(theme.panel));
        f.render_widget(list, chunks[1]);

        // + New Chat button
        let new_chat = Paragraph::new(" New chat ")
            .style(
                Style::default()
                    .fg(theme.success)
                    .add_modifier(Modifier::BOLD),
            )
            .alignment(Alignment::Center)
            .block(
                Block::default()
                    .borders(Borders::TOP)
                    .border_style(Style::default().fg(theme.border)),
            );
        f.render_widget(new_chat, chunks[2]);

        let settings = Paragraph::new(" Settings ")
            .style(Style::default().fg(theme.warning))
            .alignment(Alignment::Center)
            .block(
                Block::default()
                    .borders(Borders::TOP)
                    .border_style(Style::default().fg(theme.border)),
            );
        f.render_widget(settings, chunks[3]);
    }

    pub fn new_chat_card_area(&self, area: Rect) -> Option<Rect> {
        if !self.show_new_chat_card {
            return None;
        }
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(3),
                Constraint::Min(0),
                Constraint::Length(3),
                Constraint::Length(3),
            ])
            .split(area);
        // The "New Chat..." card is the first item in the list area
        Some(Rect::new(chunks[1].x, chunks[1].y, chunks[1].width, 1))
    }

    pub fn new_chat_button_area(&self, area: Rect) -> Rect {
        let chunks = sidebar_chunks(area);
        chunks[2]
    }

    pub fn settings_area(&self, area: Rect) -> Rect {
        let chunks = sidebar_chunks(area);
        chunks[3]
    }

    pub fn conversation_item_areas(&self, area: Rect) -> Vec<(i64, Rect)> {
        let list_area = sidebar_chunks(area)[1];
        let mut areas = Vec::with_capacity(self.conversations.len());
        for (offset, conversation) in self.conversations.iter().enumerate() {
            let row = usize::from(self.show_new_chat_card) + offset;
            if row >= list_area.height as usize {
                break;
            }
            areas.push((
                conversation.id,
                Rect::new(list_area.x, list_area.y + row as u16, list_area.width, 1),
            ));
        }

        areas
    }
}

fn sidebar_chunks(area: Rect) -> Vec<Rect> {
    Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Min(0),
            Constraint::Length(3),
            Constraint::Length(3),
        ])
        .split(area)
        .to_vec()
}
