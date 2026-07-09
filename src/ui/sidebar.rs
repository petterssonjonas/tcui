#![allow(dead_code)]
use ratatui::{prelude::*, widgets::*, Frame};

const TITLE_HEIGHT: u16 = 3;
const NEW_CHAT_HEIGHT: u16 = 3;
const SECTION_HEADER_HEIGHT: u16 = 1;
const CHAT_CARD_HEIGHT: u16 = 5;
pub const SIDEBAR_WIDTH: u16 = 28;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SidebarAction {
    NewChat,
    LoadConversation(i64),
    TogglePinned(i64),
    ExportConversation(i64),
    DeleteConversation(i64),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SidebarHitTarget {
    pub action: SidebarAction,
    pub area: Rect,
}

pub struct Sidebar<'a> {
    pub conversations: &'a [crate::ui::ConversationEntry],
    pub active_conversation: i64,
}

impl<'a> Sidebar<'a> {
    pub fn new(
        conversations: &'a [crate::ui::ConversationEntry],
        active_conversation: i64,
    ) -> Self {
        Self {
            conversations,
            active_conversation,
        }
    }

    pub fn render(&self, f: &mut Frame, area: Rect) {
        let theme = crate::theme::active_theme();
        let [title_area, new_chat_area, list_area] = sidebar_chunks(area);

        f.render_widget(
            Block::default().style(Style::default().bg(theme.sidebar)),
            area,
        );

        let title = Paragraph::new(Line::from(vec![
            Span::styled(
                " Terminal Chat UI",
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
        .style(Style::default().bg(theme.sidebar));
        f.render_widget(title, title_area);

        let new_chat = Paragraph::new(" New Chat ")
            .style(
                Style::default()
                    .fg(theme.success)
                    .bg(theme.panel)
                    .add_modifier(Modifier::BOLD),
            )
            .alignment(Alignment::Center)
            .block(
                Block::default()
                    .borders(Borders::TOP | Borders::BOTTOM)
                    .border_style(Style::default().fg(theme.border).bg(theme.panel)),
            );
        f.render_widget(new_chat, new_chat_area);

        let pinned: Vec<&crate::ui::ConversationEntry> = self
            .conversations
            .iter()
            .filter(|conversation| conversation.pinned)
            .collect();
        let recent: Vec<&crate::ui::ConversationEntry> = self
            .conversations
            .iter()
            .filter(|conversation| !conversation.pinned)
            .collect();

        let mut cursor_y = list_area.y;
        self.render_section(f, list_area, &mut cursor_y, "Pinned chats", &pinned, theme);
        self.render_section(f, list_area, &mut cursor_y, "Recent chats", &recent, theme);

        if self.conversations.is_empty() {
            let empty = Paragraph::new(" No saved chats yet")
                .style(Style::default().fg(theme.muted).bg(theme.sidebar))
                .alignment(Alignment::Left);
            f.render_widget(empty, list_area);
        }
    }

    pub fn hit_targets(&self, area: Rect) -> Vec<SidebarHitTarget> {
        let mut hits = Vec::new();
        let [_, new_chat_area, list_area] = sidebar_chunks(area);
        hits.push(SidebarHitTarget {
            action: SidebarAction::NewChat,
            area: new_chat_area,
        });

        let pinned: Vec<&crate::ui::ConversationEntry> = self
            .conversations
            .iter()
            .filter(|conversation| conversation.pinned)
            .collect();
        let recent: Vec<&crate::ui::ConversationEntry> = self
            .conversations
            .iter()
            .filter(|conversation| !conversation.pinned)
            .collect();

        let mut cursor_y = list_area.y;
        self.collect_section_hits(list_area, &mut cursor_y, &pinned, &mut hits);
        self.collect_section_hits(list_area, &mut cursor_y, &recent, &mut hits);
        hits
    }

    fn render_section(
        &self,
        f: &mut Frame,
        list_area: Rect,
        cursor_y: &mut u16,
        label: &str,
        entries: &[&crate::ui::ConversationEntry],
        theme: crate::theme::ThemeSpec,
    ) {
        let Some(header_area) = next_rect(list_area, cursor_y, SECTION_HEADER_HEIGHT) else {
            return;
        };
        let header = Paragraph::new(format!(" {label}"))
            .style(
                Style::default()
                    .fg(theme.muted)
                    .bg(theme.sidebar)
                    .add_modifier(Modifier::BOLD),
            )
            .alignment(Alignment::Left);
        f.render_widget(header, header_area);

        if entries.is_empty() {
            let Some(empty_area) = next_rect(list_area, cursor_y, 1) else {
                return;
            };
            let empty = Paragraph::new("  None")
                .style(Style::default().fg(theme.muted).bg(theme.sidebar))
                .alignment(Alignment::Left);
            f.render_widget(empty, empty_area);
            return;
        }

        for conversation in entries {
            let Some(card_area) = next_rect(list_area, cursor_y, CHAT_CARD_HEIGHT) else {
                break;
            };
            self.render_card(f, card_area, conversation, theme);
        }
    }

    fn collect_section_hits(
        &self,
        list_area: Rect,
        cursor_y: &mut u16,
        entries: &[&crate::ui::ConversationEntry],
        hits: &mut Vec<SidebarHitTarget>,
    ) {
        let Some(_) = next_rect(list_area, cursor_y, SECTION_HEADER_HEIGHT) else {
            return;
        };
        if entries.is_empty() {
            let _ = next_rect(list_area, cursor_y, 1);
            return;
        }
        for conversation in entries {
            let Some(card_area) = next_rect(list_area, cursor_y, CHAT_CARD_HEIGHT) else {
                break;
            };
            self.collect_card_hits(card_area, conversation, hits);
        }
    }

    fn render_card(
        &self,
        f: &mut Frame,
        area: Rect,
        conversation: &crate::ui::ConversationEntry,
        theme: crate::theme::ThemeSpec,
    ) {
        let active = conversation.id == self.active_conversation;
        let body_style = if active {
            theme.selected_style()
        } else {
            Style::default().fg(theme.foreground).bg(theme.sidebar)
        };
        let inner = Rect::new(
            area.x.saturating_add(1),
            area.y,
            area.width.saturating_sub(2),
            area.height,
        );
        f.render_widget(Block::default().style(body_style), area);

        if inner.height == 0 || inner.width == 0 {
            return;
        }

        let status = conversation_status(conversation, active);
        let [title_area, status_area, action_area] = card_inner_chunks(inner);
        f.render_widget(
            Paragraph::new(elide(&conversation.title, title_area.width as usize))
                .style(body_style.add_modifier(Modifier::BOLD))
                .alignment(Alignment::Left),
            title_area,
        );
        f.render_widget(
            Paragraph::new(elide(&status, status_area.width as usize))
                .style(Style::default().fg(theme.muted).bg(theme.sidebar))
                .alignment(Alignment::Left),
            status_area,
        );

        let actions = action_labels(conversation.pinned);
        let action_spans = vec![
            Span::styled(actions.pin, Style::default().fg(theme.warning)),
            Span::raw(" "),
            Span::styled(actions.export, Style::default().fg(theme.success)),
            Span::raw(" "),
            Span::styled(actions.delete, Style::default().fg(theme.error)),
        ];
        f.render_widget(
            Paragraph::new(Line::from(action_spans))
                .style(Style::default().bg(theme.sidebar))
                .alignment(Alignment::Left),
            action_area,
        );
    }

    fn collect_card_hits(
        &self,
        area: Rect,
        conversation: &crate::ui::ConversationEntry,
        hits: &mut Vec<SidebarHitTarget>,
    ) {
        let inner = Rect::new(
            area.x.saturating_add(1),
            area.y,
            area.width.saturating_sub(2),
            area.height,
        );
        if inner.height == 0 || inner.width == 0 {
            return;
        }
        let [title_area, status_area, action_area] = card_inner_chunks(inner);
        let body_height = title_area.height.saturating_add(status_area.height);
        let body_area = Rect::new(inner.x, inner.y, inner.width, body_height.max(1));
        hits.push(SidebarHitTarget {
            action: SidebarAction::LoadConversation(conversation.id),
            area: body_area,
        });

        let actions = action_labels(conversation.pinned);
        let pin_width = actions.pin.chars().count() as u16;
        let export_width = actions.export.chars().count() as u16;
        let delete_width = actions.delete.chars().count() as u16;
        let pin_area = Rect::new(action_area.x, action_area.y, pin_width, action_area.height);
        let export_x = action_area.x.saturating_add(pin_width + 1);
        let export_area = Rect::new(export_x, action_area.y, export_width, action_area.height);
        let delete_x = export_x.saturating_add(export_width + 1);
        let delete_area = Rect::new(delete_x, action_area.y, delete_width, action_area.height);

        hits.push(SidebarHitTarget {
            action: SidebarAction::TogglePinned(conversation.id),
            area: pin_area,
        });
        hits.push(SidebarHitTarget {
            action: SidebarAction::ExportConversation(conversation.id),
            area: export_area,
        });
        hits.push(SidebarHitTarget {
            action: SidebarAction::DeleteConversation(conversation.id),
            area: delete_area,
        });
    }
}

#[derive(Clone, Copy)]
struct ActionLabels {
    pin: &'static str,
    export: &'static str,
    delete: &'static str,
}

fn action_labels(pinned: bool) -> ActionLabels {
    ActionLabels {
        pin: if pinned { "[Unpin]" } else { "[Pin]" },
        export: "[Export]",
        delete: "[Del]",
    }
}

fn conversation_status(conversation: &crate::ui::ConversationEntry, active: bool) -> String {
    let mut flags = Vec::new();
    if active {
        flags.push("Active");
    }
    if conversation.pinned {
        flags.push("Pinned");
    } else {
        flags.push("Recent");
    }
    flags.join(" • ")
}

fn elide(input: &str, width: usize) -> String {
    if width == 0 {
        return String::new();
    }
    let chars: Vec<char> = input.chars().collect();
    if chars.len() <= width {
        return input.to_string();
    }
    if width == 1 {
        return "…".to_string();
    }
    let visible: String = chars.into_iter().take(width.saturating_sub(1)).collect();
    format!("{visible}…")
}

fn sidebar_chunks(area: Rect) -> [Rect; 3] {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(TITLE_HEIGHT),
            Constraint::Length(NEW_CHAT_HEIGHT),
            Constraint::Min(0),
        ])
        .split(area);
    [chunks[0], chunks[1], chunks[2]]
}

fn card_inner_chunks(area: Rect) -> [Rect; 3] {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Min(1),
        ])
        .split(area);
    [chunks[0], chunks[1], chunks[2]]
}

fn next_rect(list_area: Rect, cursor_y: &mut u16, height: u16) -> Option<Rect> {
    let bottom = list_area.bottom();
    if *cursor_y >= bottom {
        return None;
    }
    let remaining = bottom.saturating_sub(*cursor_y);
    if remaining < height {
        return None;
    }
    let rect = Rect::new(list_area.x, *cursor_y, list_area.width, height);
    *cursor_y = (*cursor_y).saturating_add(height);
    Some(rect)
}

#[cfg(test)]
mod tests {
    use super::{Sidebar, SidebarAction};
    use crate::ui::ConversationEntry;
    use ratatui::{backend::TestBackend, layout::Rect, Terminal};

    #[test]
    fn sidebar_hit_targets_include_chat_card_actions() {
        let conversations = vec![
            ConversationEntry {
                id: 1,
                title: "Pinned chat".to_string(),
                created_at: String::new(),
                updated_at_ms: 20,
                pinned: true,
            },
            ConversationEntry {
                id: 2,
                title: "Recent chat".to_string(),
                created_at: String::new(),
                updated_at_ms: 10,
                pinned: false,
            },
        ];
        let sidebar = Sidebar::new(&conversations, 2);
        let hits = sidebar.hit_targets(Rect::new(0, 0, 28, 24));

        assert!(hits.iter().any(|hit| hit.action == SidebarAction::NewChat));
        assert!(hits
            .iter()
            .any(|hit| hit.action == SidebarAction::LoadConversation(1)));
        assert!(hits
            .iter()
            .any(|hit| hit.action == SidebarAction::TogglePinned(1)));
        assert!(hits
            .iter()
            .any(|hit| hit.action == SidebarAction::ExportConversation(2)));
        assert!(hits
            .iter()
            .any(|hit| hit.action == SidebarAction::DeleteConversation(2)));
    }

    #[test]
    fn sidebar_renders_title_sections_and_actions() {
        let conversations = vec![
            ConversationEntry {
                id: 1,
                title: "Pinned chat".to_string(),
                created_at: String::new(),
                updated_at_ms: 20,
                pinned: true,
            },
            ConversationEntry {
                id: 2,
                title: "Recent chat".to_string(),
                created_at: String::new(),
                updated_at_ms: 10,
                pinned: false,
            },
        ];
        let sidebar = Sidebar::new(&conversations, 1);
        let mut terminal = Terminal::new(TestBackend::new(28, 24)).expect("terminal");

        terminal
            .draw(|frame| sidebar.render(frame, Rect::new(0, 0, 28, 24)))
            .expect("render sidebar");

        let screen = terminal.backend().to_string();
        assert!(screen.contains("Terminal Chat UI"));
        assert!(screen.contains("New Chat"));
        assert!(screen.contains("Pinned chats"));
        assert!(screen.contains("Recent chats"));
        assert!(screen.contains("[Pin]") || screen.contains("[Unpin]"));
        assert!(screen.contains("[Export]"));
        assert!(screen.contains("[Del]"));
    }
}
