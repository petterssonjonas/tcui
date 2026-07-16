#![allow(dead_code)]
use ratatui::{prelude::*, widgets::*, Frame};

const NEW_CHAT_HEIGHT: u16 = 3;
const SECTION_HEADER_HEIGHT: u16 = 1;
const CHAT_CARD_HEIGHT: u16 = 5;
const CHAT_CARD_GAP: u16 = 1;
const CHAT_CARD_MARGIN_HORIZONTAL: u16 = 1;
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SidebarSelection {
    #[default]
    NewChat,
    Conversation(i64),
}

#[derive(Debug, Clone, Default)]
pub struct SidebarState {
    pub selection: SidebarSelection,
    scroll_row: usize,
    viewport: Option<Rect>,
}

impl SidebarState {
    pub fn viewport_contains(&self, position: Position) -> bool {
        self.viewport.is_some_and(|area| area.contains(position))
    }

    pub fn scroll(&mut self, conversations: &[crate::ui::ConversationEntry], down: bool) {
        let rows = sidebar_rows(conversations);
        let visible_height = self.viewport.map_or(0, |area| area.height);
        let max_scroll = max_scroll_row(&rows, visible_height);
        self.scroll_row = if down {
            self.scroll_row.saturating_add(1).min(max_scroll)
        } else {
            self.scroll_row.saturating_sub(1)
        };
    }

    pub fn move_selection(&mut self, conversations: &[crate::ui::ConversationEntry], down: bool) {
        let selections = sidebar_selections(conversations);
        let current = selections
            .iter()
            .position(|selection| *selection == self.selection)
            .unwrap_or(0);
        let next = if down {
            current
                .saturating_add(1)
                .min(selections.len().saturating_sub(1))
        } else {
            current.saturating_sub(1)
        };
        self.selection = selections[next];
        self.ensure_selection_visible(conversations);
    }

    pub const fn selected_action(&self) -> SidebarAction {
        match self.selection {
            SidebarSelection::NewChat => SidebarAction::NewChat,
            SidebarSelection::Conversation(id) => SidebarAction::LoadConversation(id),
        }
    }

    pub fn select_action(&mut self, action: SidebarAction) {
        self.selection = match action {
            SidebarAction::NewChat => SidebarSelection::NewChat,
            SidebarAction::LoadConversation(id)
            | SidebarAction::TogglePinned(id)
            | SidebarAction::ExportConversation(id)
            | SidebarAction::DeleteConversation(id) => SidebarSelection::Conversation(id),
        };
    }

    fn prepare(&mut self, conversations: &[crate::ui::ConversationEntry], viewport: Rect) {
        self.viewport = Some(viewport);
        if let SidebarSelection::Conversation(id) = self.selection {
            if !conversations
                .iter()
                .any(|conversation| conversation.id == id)
            {
                self.selection = SidebarSelection::NewChat;
            }
        }
        let rows = sidebar_rows(conversations);
        self.scroll_row = self.scroll_row.min(max_scroll_row(&rows, viewport.height));
    }

    fn ensure_selection_visible(&mut self, conversations: &[crate::ui::ConversationEntry]) {
        let SidebarSelection::Conversation(selected_id) = self.selection else {
            return;
        };
        let rows = sidebar_rows(conversations);
        let Some(selected_row) = rows.iter().position(|row| {
            matches!(row, SidebarRow::Conversation(conversation) if conversation.id == selected_id)
        }) else {
            self.selection = SidebarSelection::NewChat;
            return;
        };
        let visible_height = self.viewport.map_or(0, |area| area.height);
        if visible_height == 0 {
            return;
        }
        if selected_row < self.scroll_row {
            self.scroll_row = selected_row;
        }
        while rendered_height(&rows, self.scroll_row, selected_row) > visible_height
            && self.scroll_row < selected_row
        {
            self.scroll_row += 1;
        }
        self.scroll_row = self.scroll_row.min(max_scroll_row(&rows, visible_height));
    }
}

pub struct Sidebar<'a> {
    conversations: &'a [crate::ui::ConversationEntry],
    active_conversation: i64,
    state: &'a mut SidebarState,
    focused: bool,
}

impl<'a> Sidebar<'a> {
    pub fn new(
        conversations: &'a [crate::ui::ConversationEntry],
        active_conversation: i64,
        state: &'a mut SidebarState,
        focused: bool,
    ) -> Self {
        Self {
            conversations,
            active_conversation,
            state,
            focused,
        }
    }

    pub fn render(&mut self, f: &mut Frame, area: Rect) -> Vec<SidebarHitTarget> {
        let theme = crate::theme::active_theme();
        let [new_chat_area, list_area] = sidebar_chunks(area);
        self.state.prepare(self.conversations, list_area);

        f.render_widget(
            Block::default().style(Style::default().bg(theme.sidebar)),
            area,
        );

        let new_chat_selected = self.focused && self.state.selection == SidebarSelection::NewChat;
        let new_chat_style = if new_chat_selected {
            theme.selected_style()
        } else {
            Style::default().fg(theme.success).bg(theme.panel)
        };
        f.render_widget(Block::default().style(new_chat_style), new_chat_area);
        f.render_widget(
            Paragraph::new("[New Chat]")
                .style(new_chat_style.add_modifier(Modifier::BOLD))
                .alignment(Alignment::Center),
            Rect::new(new_chat_area.x, new_chat_area.y + 1, new_chat_area.width, 1),
        );

        let mut hits = vec![SidebarHitTarget {
            action: SidebarAction::NewChat,
            area: new_chat_area,
        }];
        let rows = sidebar_rows(self.conversations);
        let mut cursor_y = list_area.y;
        for row in rows.iter().skip(self.state.scroll_row) {
            let row_height = row.height();
            if cursor_y.saturating_add(row_height) > list_area.bottom() {
                break;
            }
            match row {
                SidebarRow::Header(label) => {
                    f.render_widget(
                        Paragraph::new(format!(" {label}"))
                            .style(
                                Style::default()
                                    .fg(theme.muted)
                                    .bg(theme.sidebar)
                                    .add_modifier(Modifier::BOLD),
                            )
                            .alignment(Alignment::Left),
                        Rect::new(
                            list_area.x,
                            cursor_y,
                            list_area.width,
                            SECTION_HEADER_HEIGHT,
                        ),
                    );
                }
                SidebarRow::Empty => {
                    f.render_widget(
                        Paragraph::new("  None")
                            .style(Style::default().fg(theme.muted).bg(theme.sidebar)),
                        Rect::new(list_area.x, cursor_y, list_area.width, 1),
                    );
                }
                SidebarRow::Conversation(conversation) => {
                    let card_area =
                        Rect::new(list_area.x, cursor_y, list_area.width, CHAT_CARD_HEIGHT).inner(
                            Margin {
                                vertical: 0,
                                horizontal: CHAT_CARD_MARGIN_HORIZONTAL,
                            },
                        );
                    let selected = self.focused
                        && self.state.selection == SidebarSelection::Conversation(conversation.id);
                    self.render_card(f, card_area, conversation, selected, theme);
                    self.collect_card_hits(card_area, conversation, &mut hits);
                }
            }
            cursor_y = cursor_y.saturating_add(row_height);
        }

        hits
    }

    fn render_card(
        &self,
        f: &mut Frame,
        area: Rect,
        conversation: &crate::ui::ConversationEntry,
        selected: bool,
        theme: crate::theme::ThemeSpec,
    ) {
        let active = conversation.id == self.active_conversation;
        let body_style = if active || selected {
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
                .style(body_style.add_modifier(Modifier::BOLD)),
            title_area,
        );
        f.render_widget(
            Paragraph::new(elide(&status, status_area.width as usize))
                .style(body_style.fg(theme.muted)),
            status_area,
        );

        let actions = action_labels(conversation.pinned, action_area.width);
        f.render_widget(
            Paragraph::new(Line::from(vec![
                Span::styled(actions.pin, Style::default().fg(theme.warning)),
                Span::raw(" "),
                Span::styled(actions.export, Style::default().fg(theme.success)),
                Span::raw(" "),
                Span::styled(actions.delete, Style::default().fg(theme.error)),
            ]))
            .style(body_style),
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
        hits.push(SidebarHitTarget {
            action: SidebarAction::LoadConversation(conversation.id),
            area: Rect::new(inner.x, inner.y, inner.width, body_height.max(1)),
        });

        let actions = action_labels(conversation.pinned, action_area.width);
        let pin_width = actions.pin.chars().count() as u16;
        let export_width = actions.export.chars().count() as u16;
        let delete_width = actions.delete.chars().count() as u16;
        let export_x = action_area.x.saturating_add(pin_width + 1);
        let delete_x = export_x.saturating_add(export_width + 1);
        hits.extend([
            SidebarHitTarget {
                action: SidebarAction::TogglePinned(conversation.id),
                area: Rect::new(action_area.x, action_area.y, pin_width, action_area.height),
            },
            SidebarHitTarget {
                action: SidebarAction::ExportConversation(conversation.id),
                area: Rect::new(export_x, action_area.y, export_width, action_area.height),
            },
            SidebarHitTarget {
                action: SidebarAction::DeleteConversation(conversation.id),
                area: Rect::new(delete_x, action_area.y, delete_width, action_area.height),
            },
        ]);
    }
}

enum SidebarRow<'a> {
    Header(&'static str),
    Empty,
    Conversation(&'a crate::ui::ConversationEntry),
}

impl SidebarRow<'_> {
    const fn height(&self) -> u16 {
        match self {
            Self::Header(_) | Self::Empty => 1,
            Self::Conversation(_) => CHAT_CARD_HEIGHT + CHAT_CARD_GAP,
        }
    }
}

fn sidebar_rows(conversations: &[crate::ui::ConversationEntry]) -> Vec<SidebarRow<'_>> {
    let mut rows = Vec::with_capacity(conversations.len() + 4);
    rows.push(SidebarRow::Header("Pinned chats"));
    let mut pinned = conversations
        .iter()
        .filter(|conversation| conversation.pinned);
    if let Some(first) = pinned.next() {
        rows.push(SidebarRow::Conversation(first));
        rows.extend(pinned.map(SidebarRow::Conversation));
    } else {
        rows.push(SidebarRow::Empty);
    }
    rows.push(SidebarRow::Header("Recent chats"));
    let mut recent = conversations
        .iter()
        .filter(|conversation| !conversation.pinned);
    if let Some(first) = recent.next() {
        rows.push(SidebarRow::Conversation(first));
        rows.extend(recent.map(SidebarRow::Conversation));
    } else {
        rows.push(SidebarRow::Empty);
    }
    rows
}

fn sidebar_selections(conversations: &[crate::ui::ConversationEntry]) -> Vec<SidebarSelection> {
    std::iter::once(SidebarSelection::NewChat)
        .chain(
            conversations
                .iter()
                .filter(|conversation| conversation.pinned)
                .chain(
                    conversations
                        .iter()
                        .filter(|conversation| !conversation.pinned),
                )
                .map(|conversation| SidebarSelection::Conversation(conversation.id)),
        )
        .collect()
}

fn max_scroll_row(rows: &[SidebarRow<'_>], visible_height: u16) -> usize {
    let mut remaining = rows.iter().map(SidebarRow::height).sum::<u16>();
    let mut offset = 0;
    while remaining > visible_height && offset + 1 < rows.len() {
        remaining = remaining.saturating_sub(rows[offset].height());
        offset += 1;
    }
    offset
}

fn rendered_height(rows: &[SidebarRow<'_>], start: usize, end: usize) -> u16 {
    rows.get(start..=end)
        .unwrap_or_default()
        .iter()
        .map(SidebarRow::height)
        .sum()
}

#[derive(Clone, Copy)]
struct ActionLabels {
    pin: &'static str,
    export: &'static str,
    delete: &'static str,
}

const fn action_labels(pinned: bool, width: u16) -> ActionLabels {
    let full_width = if pinned { 22 } else { 20 };
    let compact_width = if pinned { 19 } else { 17 };
    if width >= full_width {
        ActionLabels {
            pin: if pinned { "[Unpin]" } else { "[Pin]" },
            export: "[Export]",
            delete: "[Del]",
        }
    } else if width >= compact_width {
        ActionLabels {
            pin: if pinned { "[Unpin]" } else { "[Pin]" },
            export: "[Exp]",
            delete: "[Del]",
        }
    } else {
        ActionLabels {
            pin: "[P]",
            export: "[E]",
            delete: "[D]",
        }
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

fn sidebar_chunks(area: Rect) -> [Rect; 2] {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(NEW_CHAT_HEIGHT), Constraint::Min(0)])
        .split(area);
    [chunks[0], chunks[1]]
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

#[cfg(test)]
mod tests {
    use super::{Sidebar, SidebarAction, SidebarSelection, SidebarState};
    use crate::ui::ConversationEntry;
    use ratatui::{backend::TestBackend, layout::Rect, Terminal};

    fn conversation(id: i64, pinned: bool) -> ConversationEntry {
        ConversationEntry {
            id,
            title: format!("Chat {id}"),
            created_at: String::new(),
            updated_at_ms: id,
            pinned,
        }
    }

    #[test]
    fn sidebar_renders_compact_new_chat_without_legacy_title() {
        let conversations = vec![conversation(1, true), conversation(2, false)];
        let mut state = SidebarState::default();
        let mut terminal = Terminal::new(TestBackend::new(28, 24)).expect("terminal");

        terminal
            .draw(|frame| {
                Sidebar::new(&conversations, 1, &mut state, false)
                    .render(frame, Rect::new(0, 0, 28, 24));
            })
            .expect("render sidebar");

        let screen = terminal.backend().to_string();
        assert!(!screen.contains("Terminal Chat UI"));
        assert!(screen.contains("[New Chat]"));
        assert!(screen.contains("Pinned chats"));
        assert!(screen.contains("Recent chats"));
    }

    #[test]
    fn sidebar_scroll_reaches_last_conversation_and_moves_hit_targets() {
        let conversations = (1..=8)
            .map(|id| conversation(id, false))
            .collect::<Vec<_>>();
        let mut state = SidebarState::default();
        let mut terminal = Terminal::new(TestBackend::new(28, 14)).expect("terminal");
        let area = Rect::new(0, 0, 28, 14);

        terminal
            .draw(|frame| {
                Sidebar::new(&conversations, 1, &mut state, true).render(frame, area);
            })
            .expect("render initial sidebar");
        for _ in 0..16 {
            state.scroll(&conversations, true);
        }
        let mut hits = Vec::new();
        terminal
            .draw(|frame| {
                hits = Sidebar::new(&conversations, 1, &mut state, true).render(frame, area);
            })
            .expect("render scrolled sidebar");

        assert!(hits
            .iter()
            .any(|hit| hit.action == SidebarAction::LoadConversation(8)));
    }

    #[test]
    fn sidebar_keyboard_selection_uses_pinned_then_recent_order() {
        let conversations = vec![
            conversation(1, false),
            conversation(2, true),
            conversation(3, false),
        ];
        let mut state = SidebarState::default();

        state.move_selection(&conversations, true);
        assert_eq!(state.selection, SidebarSelection::Conversation(2));
        state.move_selection(&conversations, true);
        assert_eq!(state.selection, SidebarSelection::Conversation(1));
        state.move_selection(&conversations, true);
        assert_eq!(state.selection, SidebarSelection::Conversation(3));
    }

    #[test]
    fn pinned_actions_fit_default_twenty_four_column_sidebar() {
        let conversations = vec![conversation(1, true)];
        let mut state = SidebarState::default();
        let mut terminal = Terminal::new(TestBackend::new(24, 14)).expect("terminal");
        let mut hits = Vec::new();

        terminal
            .draw(|frame| {
                hits = Sidebar::new(&conversations, 1, &mut state, false)
                    .render(frame, Rect::new(0, 0, 24, 14));
            })
            .expect("render narrow sidebar");

        let screen = terminal.backend().to_string();
        assert!(screen.contains("[Unpin] [Exp] [Del]"));
        assert!(hits.iter().all(|hit| hit.area.right() <= 24));
    }
}
