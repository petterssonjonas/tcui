//! Settings palette: registry, fuzzy search, grouping, pin/unpin, overlay render.
#![allow(dead_code)]

use crate::app::Action;
use crate::tui::components::{centered_rect, GroupHeader, SearchField, SelectList};
use nucleo_matcher::{Config, Matcher, Utf32Str};
use ratatui::{prelude::*, widgets::*, Frame};

use CommandCategory as C;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CommandCategory {
    System,
    Session,
    Prompt,
    Provider,
    Agent,
    Mcps,
    Other,
}

impl CommandCategory {
    pub fn label(self) -> &'static str {
        match self {
            Self::System => "System",
            Self::Session => "Session",
            Self::Prompt => "Prompt",
            Self::Provider => "Provider",
            Self::Agent => "Agent",
            Self::Mcps => "MCPs",
            Self::Other => "Other",
        }
    }
    fn group_order(self) -> u8 {
        match self {
            Self::System => 0,
            Self::Session => 1,
            Self::Prompt => 2,
            Self::Provider => 3,
            Self::Agent => 4,
            Self::Mcps => 5,
            Self::Other => 6,
        }
    }
}

#[derive(Debug, Clone)]
pub struct Command {
    pub id: String,
    pub title: String,
    pub description: String,
    pub category: CommandCategory,
    pub keywords: Vec<String>,
    pub shortcut: Option<String>,
    pub action: Action,
    pub curated: bool,
}

#[derive(Debug, Clone)]
pub struct PaletteState {
    search: SearchField,
    list: SelectList<String>,
    commands: Vec<Command>,
    pinned: Vec<String>,
    scroll_offset: usize,
}

impl PaletteState {
    pub fn new(pinned: Vec<String>) -> Self {
        let mut s = Self {
            search: SearchField::new(),
            list: SelectList::new(Vec::new()),
            commands: all_commands(),
            pinned,
            scroll_offset: 0,
        };
        s.rebuild();
        s
    }

    pub fn query(&self) -> &str {
        self.search.query()
    }
    pub fn list(&self) -> &SelectList<String> {
        &self.list
    }
    pub fn pinned(&self) -> &[String] {
        &self.pinned
    }
    pub fn set_pinned(&mut self, pinned: Vec<String>) {
        self.pinned = pinned;
        if self.search.query().trim().is_empty() {
            self.rebuild();
        }
    }
    pub fn insert_char(&mut self, c: char) {
        self.search.insert(&c.to_string());
        self.scroll_offset = 0;
        self.rebuild();
    }
    pub fn backspace(&mut self) {
        self.search.backspace();
        self.scroll_offset = 0;
        self.rebuild();
    }
    pub fn move_up(&mut self) {
        self.list.up();
        if self.list.selected() < self.scroll_offset {
            self.scroll_offset = self.list.selected();
        }
    }
    pub fn move_down(&mut self) {
        self.list.down();
    }

    pub fn toggle_pin(&mut self) -> bool {
        let Some(id) = self.list.selected_item().cloned() else {
            return false;
        };
        if let Some(idx) = self.pinned.iter().position(|p| p == &id) {
            self.pinned.remove(idx);
        } else {
            self.pinned.push(id);
        }
        if self.search.query().trim().is_empty() {
            self.rebuild();
        }
        true
    }

    fn rebuild(&mut self) {
        self.scroll_offset = 0;
        self.list = SelectList::new(if self.search.query().trim().is_empty() {
            self.empty_query_ids()
        } else {
            self.search_ids(self.search.query())
        });
    }

    fn empty_query_ids(&self) -> Vec<String> {
        let mut ids = Vec::new();
        for c in &self.commands {
            if c.curated {
                ids.push(c.id.clone());
            }
        }
        for pin in &self.pinned {
            if self.commands.iter().any(|c| c.id == *pin && c.curated) {
                continue;
            }
            if self.commands.iter().any(|c| c.id == *pin) {
                ids.push(pin.clone());
            }
        }
        let mut rest: Vec<&Command> = self
            .commands
            .iter()
            .filter(|c| !c.curated && !self.pinned.contains(&c.id))
            .collect();
        rest.sort_by_key(|c| c.category.group_order());
        for c in rest {
            ids.push(c.id.clone());
        }
        ids
    }

    fn search_ids(&self, query: &str) -> Vec<String> {
        if query.trim().is_empty() {
            return self.commands.iter().map(|c| c.id.clone()).collect();
        }
        let qchars: Vec<char> = query.to_lowercase().chars().collect();
        let needle = Utf32Str::Unicode(&qchars);
        let mut matcher = Matcher::new(Config::DEFAULT);
        let mut scored: Vec<(f64, String)> = Vec::new();
        for c in &self.commands {
            let s = fuzzy_score(needle, c, &mut matcher);
            if s > 0.0 {
                scored.push((s, c.id.clone()));
            }
        }
        scored.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));
        scored.into_iter().map(|(_, id)| id).collect()
    }

    pub fn results(&self) -> Vec<(usize, &Command)> {
        self.list
            .items()
            .iter()
            .filter_map(|id| {
                self.commands
                    .iter()
                    .position(|c| &c.id == id)
                    .map(|i| (i, &self.commands[i]))
            })
            .collect()
    }
    pub fn selected_command(&self) -> Option<&Command> {
        self.list
            .selected_item()
            .and_then(|id| self.commands.iter().find(|c| &c.id == id))
    }
    pub fn selected_action(&self) -> Option<Action> {
        self.selected_command().map(|c| c.action.clone())
    }
    pub fn select(&mut self, index: usize) {
        if index < self.list.items().len() {
            self.list.select_at(index);
        }
    }
    pub fn visible_item_areas(&self, area: Rect) -> Vec<(Rect, usize)> {
        let popup = centered_rect(70, 60, area);
        let inner = popup.inner(Margin {
            vertical: 1,
            horizontal: 1,
        });
        let list_area = Rect::new(
            inner.x,
            inner.y + 2,
            inner.width,
            inner.height.saturating_sub(2),
        );
        let items = self.list.items();
        if items.is_empty() {
            return Vec::new();
        }
        let selected = self.list.selected();
        let empty_q = self.search.query().trim().is_empty();
        let bottom = list_area.y + list_area.height;
        let visible_rows = list_area.height as usize;
        let scroll_offset = if visible_rows == 0 {
            0
        } else if selected < self.scroll_offset {
            selected
        } else if selected >= self.scroll_offset + visible_rows {
            selected.saturating_sub(visible_rows - 1)
        } else {
            self.scroll_offset
        };
        let mut areas = Vec::new();
        let mut y = if empty_q {
            list_area.y.saturating_add(1)
        } else {
            list_area.y
        };
        let mut last_group: Option<&str> = None;
        for (idx, id) in items.iter().enumerate().skip(scroll_offset) {
            if y >= bottom {
                break;
            }
            let Some(cmd) = self.find_command(id) else {
                continue;
            };
            if empty_q {
                let group = if cmd.curated {
                    "Curated"
                } else if self.is_pinned(&cmd.id) {
                    "Pinned"
                } else {
                    cmd.category.label()
                };
                if Some(group) != last_group {
                    last_group = Some(group);
                    y = y.saturating_add(1);
                    if y >= bottom {
                        break;
                    }
                }
            }
            areas.push((Rect::new(list_area.x, y, list_area.width, 1), idx));
            y = y.saturating_add(1);
        }
        areas
    }
    pub fn is_pinned(&self, id: &str) -> bool {
        self.pinned.iter().any(|p| p == id)
    }
    pub fn find_command(&self, id: &str) -> Option<&Command> {
        self.commands.iter().find(|c| c.id == id)
    }

    #[rustfmt::skip]
    pub fn render(&self, f: &mut Frame, area: Rect) {
        let theme = crate::theme::active_theme();
        let popup = centered_rect(70, 60, area);
        f.render_widget(Clear, popup);
        f.render_widget(
            Block::default().style(Style::default().bg(theme.panel)),
            popup,
        );
        f.render_widget(
            Paragraph::new(Line::from(" Settings ").style(Style::default().fg(theme.accent).add_modifier(Modifier::BOLD))),
            Rect::new(popup.x, popup.y, popup.width, 1),
        );
        let inner = popup.inner(Margin { vertical: 1, horizontal: 1 });
        f.render_widget(
            Paragraph::new(format!("> {}", self.search.query())).style(Style::default().fg(Color::Yellow)),
            Rect::new(inner.x, inner.y, inner.width, 1),
        );
        let list_area = Rect::new(inner.x, inner.y + 2, inner.width, inner.height.saturating_sub(2));
        let items = self.list.items();
        let selected = self.list.selected();
        if items.is_empty() {
            let help = if self.search.is_empty() { "No commands available" } else { "No matches found" };
            f.render_widget(Paragraph::new(help).alignment(Alignment::Center).style(Style::default().fg(Color::DarkGray)), list_area);
            return;
        }
        let empty_q = self.search.query().trim().is_empty();
        let bottom = list_area.y + list_area.height;
        let mut y = list_area.y;
        let mut last_group: Option<&str> = None;
        let visible_rows = list_area.height as usize;

        // Pre-pass: simulate rendering from current scroll_offset to find the
        // actual last visible item index, accounting for group header rows.
        // Group headers consume vertical space, reducing effective item capacity.
        let header_overhead = if empty_q { 1 } else { 0 }; // hint line
        let mut sim_y = header_overhead;
        let mut sim_last_group: Option<&str> = None;
        let mut last_visible = self.scroll_offset;
        for (idx, id) in items.iter().enumerate().skip(self.scroll_offset) {
            if sim_y >= visible_rows {
                break;
            }
            if empty_q {
                if let Some(cmd) = self.find_command(id) {
                    let g = if cmd.curated {
                        "Curated"
                    } else if self.is_pinned(&cmd.id) {
                        "Pinned"
                    } else {
                        cmd.category.label()
                    };
                    if Some(g) != sim_last_group {
                        sim_last_group = Some(g);
                        sim_y += 1;
                        if sim_y >= visible_rows {
                            break;
                        }
                    }
                }
            }
            last_visible = idx;
            sim_y += 1;
        }

        let scroll_offset = if visible_rows == 0 {
            0
        } else if selected < self.scroll_offset {
            // Selected went above viewport — scroll up to show it
            selected
        } else if selected > last_visible {
            // Selected went below viewport — scroll down so it's near the bottom
            // Walk backwards from selected to find an offset that fits
            let mut new_offset = selected;
            let mut fill = 1; // selected itself takes 1 row
            let mut prev_group: Option<&str> = None;
            let mut check_idx = selected;
            while check_idx > 0 && fill < visible_rows.saturating_sub(header_overhead) {
                check_idx -= 1;
                if let Some(cmd) = self.find_command(&items[check_idx]) {
                    if empty_q {
                        let g = if cmd.curated {
                            "Curated"
                        } else if self.is_pinned(&cmd.id) {
                            "Pinned"
                        } else {
                            cmd.category.label()
                        };
                        if Some(g) != prev_group {
                            prev_group = Some(g);
                            fill += 1; // header row
                        }
                    }
                }
                fill += 1;
                if fill < visible_rows.saturating_sub(header_overhead) {
                    new_offset = check_idx;
                }
            }
            new_offset
        } else {
            self.scroll_offset
        };
        if empty_q {
            f.render_widget(
                Paragraph::new("Ctrl+F to pin/unpin").style(Style::default().fg(Color::DarkGray)),
                Rect::new(list_area.x, list_area.y, list_area.width, 1),
            );
            y = y.saturating_add(1);
        }
        for (idx, id) in items.iter().enumerate().skip(scroll_offset) {
            if y >= bottom { break; }
            let Some(cmd) = self.find_command(id) else { continue };
            if empty_q {
                let g = if cmd.curated { "Curated" } else if self.is_pinned(&cmd.id) { "Pinned" } else { cmd.category.label() };
                if Some(g) != last_group {
                    let _h = GroupHeader(g);
                    f.render_widget(
                        Paragraph::new(format!(" {} ", g)).style(Style::default().fg(Color::Blue).add_modifier(Modifier::BOLD)),
                        Rect::new(list_area.x, y, list_area.width, 1),
                    );
                    last_group = Some(g);
                    y += 1;
                    if y >= bottom { break; }
                }
            }
            let style = if idx == selected { Style::default().bg(Color::DarkGray).fg(Color::White) } else { Style::default() };
            let row_width = list_area.width as usize;
            let line = match &cmd.shortcut {
                Some(sc) => {
                    let title = format!(" {}", cmd.title);
                    let shortcut = format!(" {sc} ");
                    let gap = row_width.saturating_sub(title.len() + shortcut.len());
                    Line::from(vec![
                        Span::styled(title, style),
                        Span::styled(" ".repeat(gap), style),
                        Span::styled(shortcut, style.fg(Color::DarkGray)),
                    ])
                }
                None => Line::styled(format!(" {}", cmd.title), style),
            };
            f.render_widget(Paragraph::new(line), Rect::new(list_area.x, y, list_area.width, 1));
            y += 1;
        }
    }
}

fn fuzzy_score(needle: Utf32Str, cmd: &Command, m: &mut Matcher) -> f64 {
    let title: Vec<char> = cmd.title.to_lowercase().chars().collect();
    let t = m
        .fuzzy_match(Utf32Str::Unicode(&title), needle)
        .map(|s| s as f64)
        .unwrap_or(0.0);
    let desc: Vec<char> = cmd.description.to_lowercase().chars().collect();
    let d = m
        .fuzzy_match(Utf32Str::Unicode(&desc), needle)
        .map(|s| s as f64)
        .unwrap_or(0.0);
    let mut best_kw = 0.0;
    for kw in &cmd.keywords {
        let k: Vec<char> = kw.to_lowercase().chars().collect();
        let s = m
            .fuzzy_match(Utf32Str::Unicode(&k), needle)
            .map(|s| s as f64)
            .unwrap_or(0.0);
        if s > best_kw {
            best_kw = s;
        }
    }
    let mut best: f64 = 0.0;
    if t > 0.0 {
        best = best.max(3.0 + t);
    }
    if d > 0.0 {
        best = best.max(2.0 + d);
    }
    if best_kw > 0.0 {
        best = best.max(1.0 + best_kw);
    }
    best
}

pub fn has_duplicate_ids(commands: &[Command]) -> bool {
    let mut seen = std::collections::HashSet::new();
    commands.iter().any(|c| !seen.insert(&c.id))
}

#[rustfmt::skip]
#[allow(clippy::type_complexity)]
pub fn all_commands() -> Vec<Command> {
    let rows: Vec<(&str, &str, CommandCategory, Option<&str>, Action, bool)> = vec![
        ("open_settings", "/settings", C::System, Some("Ctrl+,"), Action::ShowSettings, true),
        ("open_settings_panel", "Open Settings Panel", C::System, None, Action::OpenSettingsPanel, false),
        ("toggle_settings", "Toggle Settings", C::System, Some("Ctrl+S"), Action::ToggleSettings, false),
        ("new_chat", "New Chat", C::Session, Some("Ctrl+N"), Action::NewChat, true),
        ("close_chat", "Close Chat", C::Session, Some("Ctrl+Shift+W"), Action::CloseChat, false),
        ("toggle_sidebar", "Toggle Sidebar", C::System, Some("Ctrl+B"), Action::ToggleSidebar, false),
        ("toggle_artifact_sidebar", "Toggle Artifact Sidebar", C::System, Some("Ctrl+]"), Action::ToggleArtifactSidebar, false),
        ("toggle_session_list", "Toggle Session List", C::Session, None, Action::ToggleSessionList, false),
        ("refresh_models", "Refresh Models", C::Provider, Some("Ctrl+R"), Action::RefreshModels, false),
        ("toggle_web_search", "Toggle Web Search", C::Prompt, None, Action::ToggleWebSearch, false),
        ("export_conversation", "Export Conversation", C::Session, None, Action::ExportConversation, false),
        ("show_skills", "Show Skills", C::Agent, None, Action::ShowSkillsPopup, false),
        ("show_mcp", "Show MCP", C::Mcps, None, Action::ShowMcpPopup, false),
        ("show_help", "/help", C::System, None, Action::ShowHelp, false),
        ("quit", "Quit", C::System, Some("Ctrl+Q"), Action::Quit, false),
        ("show_local_search", "Search Vault", C::Prompt, None, Action::ShowLocalSearch(String::new()), false),
        ("focus_input", "Focus Input", C::System, None, Action::FocusInput, false),
    ];
    rows.into_iter().map(|(id, title, cat, sc, act, cur)| Command {
        id: id.into(), title: title.into(), description: title.into(),
        category: cat, keywords: Vec::new(), shortcut: sc.map(|s| s.into()), action: act, curated: cur,
    }).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ranking_set_finds_open_settings_before_quit() {
        let p = PaletteState::new(vec![]);
        let r = p.search_ids("set");
        let o = r.iter().position(|x| x == "open_settings");
        let q = r.iter().position(|x| x == "quit");
        assert!(o.is_some(), "'set' must match open_settings");
        if let (Some(o), Some(q)) = (o, q) {
            assert!(o < q, "open_settings must rank before quit");
        }
    }

    #[test]
    fn empty_query_curated_then_pinned_then_category() {
        let p = PaletteState::new(vec!["quit".to_string()]);
        let ids = p.empty_query_ids();
        assert_eq!(ids.first().map(|s| s.as_str()), Some("open_settings"));
        let quit_pos = ids.iter().position(|x| x == "quit").unwrap();
        let toggle_pos = ids.iter().position(|x| x == "toggle_settings").unwrap();
        assert!(quit_pos < toggle_pos, "pinned must precede category groups");
    }

    #[test]
    fn toggle_pin_adds_then_removes() {
        let mut p = PaletteState::new(vec![]);
        assert!(p.pinned().is_empty());
        assert!(p.toggle_pin());
        let id = p.list().selected_item().unwrap().clone();
        assert_eq!(p.pinned(), &[id]);
        assert!(p.toggle_pin());
        assert!(p.pinned().is_empty());
    }

    #[test]
    fn duplicate_ids_detected() {
        let mut cmds = all_commands();
        cmds.push(Command {
            id: "quit".into(),
            title: "Dup".into(),
            description: "dup".into(),
            category: CommandCategory::System,
            keywords: Vec::new(),
            shortcut: None,
            action: Action::Quit,
            curated: false,
        });
        assert!(has_duplicate_ids(&cmds));
    }

    #[test]
    fn default_registry_no_duplicates() {
        assert!(!has_duplicate_ids(&all_commands()));
    }

    #[test]
    fn search_empty_returns_all() {
        let p = PaletteState::new(vec![]);
        assert!(!p.search_ids("").is_empty());
    }
}
