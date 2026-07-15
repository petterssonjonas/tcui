// TODO: Infrastructure for provider/model/local/MCP/keybind settings is preserved
// here (SettingType variants, confirm modals, keybind helpers) but not yet wired
// to live data. Revisit when adding those setting categories back.
#![allow(dead_code)]

use std::collections::{BTreeMap, HashMap};

use crate::tui::components::{centered_rect, ConfirmModal, GroupHeader, SearchField};
use crate::tui::focus::Focus;
use ratatui::{prelude::*, widgets::*, Frame};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SettingCategory {
    Provider,
    Model,
    Local,
    Theme,
    Keybind,
    Mcp,
    Obsidian,
    UiLayout,
    ChatBehavior,
    Privacy,
    Experimental,
    Reset,
}

impl SettingCategory {
    #[rustfmt::skip]
    pub const ALL: [Self; 12] = [Self::Provider, Self::Model, Self::Local, Self::Theme, Self::Keybind, Self::Mcp, Self::Obsidian, Self::UiLayout, Self::ChatBehavior, Self::Privacy, Self::Experimental, Self::Reset];

    pub fn label(self) -> &'static str {
        match self {
            Self::Provider => "Provider",
            Self::Model => "Model",
            Self::Local => "Local",
            Self::Theme => "Theme",
            Self::Keybind => "Keybind",
            Self::Mcp => "MCP",
            Self::Obsidian => "Obsidian",
            Self::UiLayout => "UI Layout",
            Self::ChatBehavior => "Chat Behavior",
            Self::Privacy => "Privacy",
            Self::Experimental => "Experimental",
            Self::Reset => "Reset",
        }
    }

    fn color(self) -> Color {
        match self {
            Self::Provider => Color::Cyan,
            Self::Model => Color::Blue,
            Self::Local => Color::Green,
            Self::Theme => Color::Magenta,
            Self::Keybind => Color::Yellow,
            Self::Mcp => Color::LightCyan,
            Self::Obsidian => Color::LightMagenta,
            Self::UiLayout => Color::LightBlue,
            Self::ChatBehavior => Color::LightGreen,
            Self::Privacy => Color::Gray,
            Self::Experimental => Color::LightYellow,
            Self::Reset => Color::Red,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DangerLevel {
    Safe,
    Warning,
    Danger,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SettingType {
    Subsection,
    Bool {
        enabled: bool,
    },
    Keybind {
        action_id: &'static str,
        default_binding: &'static str,
        reserved: bool,
    },
    Text,
    Number,
    Choice(&'static [&'static str]),
    Theme(&'static str),
    ToastPosition(crate::config::ToastPosition),
    DestructiveAction,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Setting {
    pub id: &'static str,
    pub title: &'static str,
    pub description: &'static str,
    pub category: SettingCategory,
    pub setting_type: SettingType,
    pub keywords: &'static [&'static str],
    pub danger: DangerLevel,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EnterResult {
    Nothing,
    EnteredSubsection,
    ToggledBool,
    OpenKeybind {
        action_id: &'static str,
        action_label: &'static str,
    },
    SelectTheme(&'static str),
    SelectToastPosition(crate::config::ToastPosition),
    RequestConfirm,
}

#[derive(Debug, Clone, Default)]
pub struct SettingsPanelState {
    pub search: SearchField,
    pub selected: usize,
    pub depth_stack: Vec<SettingCategory>,
    pub confirm: Option<ConfirmModal>,
    pub bool_toggles: HashMap<String, bool>,
    pub confirm_selected: bool,
    pub scroll_offset: usize,
}

impl SettingsPanelState {
    pub fn new() -> Self {
        Self {
            search: SearchField::new(),
            selected: 0,
            depth_stack: Vec::new(),
            confirm: None,
            bool_toggles: HashMap::new(),
            confirm_selected: true,
            scroll_offset: 0,
        }
    }

    pub fn query(&self) -> &str {
        self.search.query()
    }

    pub fn insert_char(&mut self, c: char) {
        self.search.insert(&c.to_string());
        self.selected = 0;
        self.scroll_offset = 0;
    }

    pub fn backspace(&mut self) {
        self.search.backspace();
        self.selected = 0;
        self.scroll_offset = 0;
    }

    pub fn move_up(&mut self) {
        self.selected = self.selected.saturating_sub(1);
        if self.selected < self.scroll_offset {
            self.scroll_offset = self.selected;
        }
    }

    pub fn move_down(&mut self, settings: &[Setting]) {
        let count = self.results(settings).len();
        if self.selected + 1 < count {
            self.selected += 1;
        }
    }

    pub fn enter(&mut self, settings: &[Setting]) -> EnterResult {
        let Some((_, setting)) = self.results(settings).get(self.selected).copied() else {
            return EnterResult::Nothing;
        };
        match setting.setting_type {
            SettingType::Subsection => {
                self.depth_stack.push(setting.category);
                self.selected = 0;
                self.search.clear();
                EnterResult::EnteredSubsection
            }
            SettingType::Bool { enabled: _ } => {
                self.toggle_bool_setting(setting);
                EnterResult::ToggledBool
            }
            SettingType::Keybind { action_id, .. } => EnterResult::OpenKeybind {
                action_id,
                action_label: setting.title,
            },
            SettingType::Theme(key) => EnterResult::SelectTheme(key),
            SettingType::ToastPosition(position) => EnterResult::SelectToastPosition(position),
            SettingType::DestructiveAction => {
                self.confirm = Some(
                    ConfirmModal::new(setting.title, setting.description)
                        .with_danger(setting.danger == DangerLevel::Danger),
                );
                self.confirm_selected = true;
                EnterResult::RequestConfirm
            }
            SettingType::Text | SettingType::Number | SettingType::Choice(_) => {
                EnterResult::Nothing
            }
        }
    }

    pub fn toggle_bool(&mut self, settings: &[Setting]) -> bool {
        let selected = self.selected;
        let rows = self.results(settings);
        let Some((_, setting)) = rows.get(selected).copied() else {
            return false;
        };
        if !matches!(setting.setting_type, SettingType::Bool { .. }) {
            return false;
        }
        self.toggle_bool_setting(setting);
        true
    }

    pub fn selected_setting<'a>(&self, settings: &'a [Setting]) -> Option<&'a Setting> {
        self.results(settings)
            .get(self.selected)
            .map(|(_, setting)| *setting)
    }

    pub fn select(&mut self, index: usize, settings: &[Setting]) {
        if index < self.results(settings).len() {
            self.selected = index;
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
        self.visible_item_areas_in(list_area)
    }

    fn visible_item_areas_in(&self, area: Rect) -> Vec<(Rect, usize)> {
        let settings = all_settings();
        let rows = self.results(&settings);
        if rows.is_empty() {
            return Vec::new();
        }
        let mut areas = Vec::new();
        let mut y = area.y;
        let bottom = area.y + area.height;
        let mut last_group = None;
        let visible_rows = area.height as usize;
        let scroll_offset = if visible_rows == 0 {
            0
        } else if self.selected < self.scroll_offset {
            self.selected
        } else if self.selected >= self.scroll_offset + visible_rows {
            self.selected.saturating_sub(visible_rows - 1)
        } else {
            self.scroll_offset
        };
        for (visible_idx, (_, setting)) in rows.iter().enumerate().skip(scroll_offset) {
            if y >= bottom {
                break;
            }
            if self.current_category().is_none() && Some(setting.category) != last_group {
                last_group = Some(setting.category);
                y = y.saturating_add(1);
                if y >= bottom {
                    break;
                }
            }
            areas.push((Rect::new(area.x, y, area.width, 1), visible_idx));
            y = y.saturating_add(1);
        }
        areas
    }

    fn toggle_bool_setting(&mut self, setting: &Setting) {
        let SettingType::Bool { enabled } = setting.setting_type else {
            return;
        };
        let current = self
            .bool_toggles
            .get(setting.id)
            .copied()
            .unwrap_or(enabled);
        self.bool_toggles.insert(setting.id.to_string(), !current);
    }

    pub fn esc(&mut self) -> bool {
        if self.confirm.take().is_some() {
            return false;
        }
        if self.depth_stack.pop().is_some() {
            self.selected = 0;
            false
        } else {
            true
        }
    }

    pub fn toggle_confirm_selection(&mut self) {
        self.confirm_selected = !self.confirm_selected;
    }

    pub fn confirm_reset(&mut self) -> bool {
        if self.confirm.take().is_none() {
            return false;
        }
        if self.confirm_selected {
            self.bool_toggles.clear();
            self.search.clear();
            self.selected = 0;
            self.scroll_offset = 0;
            self.depth_stack.clear();
            true
        } else {
            false
        }
    }

    pub fn results<'a>(&self, settings: &'a [Setting]) -> Vec<(usize, &'a Setting)> {
        let query = self.search.query().trim().to_lowercase();
        settings
            .iter()
            .enumerate()
            .filter(|(_, setting)| self.in_scope(setting))
            .filter(|(_, setting)| query.is_empty() || setting_matches(setting, &query))
            .collect()
    }

    pub fn current_category(&self) -> Option<SettingCategory> {
        self.depth_stack.last().copied()
    }

    pub fn render(&self, f: &mut Frame, area: Rect) {
        self.render_with_keybindings(f, area, &BTreeMap::new());
    }

    pub fn render_with_keybindings(
        &self,
        f: &mut Frame,
        area: Rect,
        keybinding_overrides: &BTreeMap<String, String>,
    ) {
        let _focus = Focus::SettingsPanel;
        let theme = crate::theme::active_theme();
        let popup = centered_rect(70, 60, area);
        f.render_widget(Clear, popup);
        f.render_widget(
            Block::default().style(Style::default().bg(theme.panel)),
            popup,
        );
        f.render_widget(
            Paragraph::new(" Settings ").style(
                Style::default()
                    .fg(theme.accent)
                    .add_modifier(Modifier::BOLD),
            ),
            Rect::new(popup.x + 1, popup.y, popup.width.saturating_sub(2), 1),
        );

        let inner = popup.inner(Margin {
            vertical: 1,
            horizontal: 1,
        });
        f.render_widget(
            Paragraph::new(format!("> {}", self.search.query()))
                .style(Style::default().fg(theme.warning)),
            Rect::new(inner.x, inner.y, inner.width, 1),
        );
        self.render_results(
            f,
            Rect::new(
                inner.x,
                inner.y + 2,
                inner.width,
                inner.height.saturating_sub(2),
            ),
            keybinding_overrides,
        );

        if let Some(confirm) = &self.confirm {
            self.render_confirm(f, area, confirm);
        }
    }

    fn render_confirm(&self, f: &mut Frame, area: Rect, confirm: &ConfirmModal) {
        let theme = crate::theme::active_theme();
        let popup = centered_rect(42, 22, area);
        f.render_widget(Clear, popup);

        let yes_style = if self.confirm_selected {
            Style::default()
                .fg(Color::Black)
                .bg(Color::Green)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(Color::Green)
        };
        let no_style = if self.confirm_selected {
            Style::default().fg(Color::Red)
        } else {
            Style::default()
                .fg(Color::Black)
                .bg(Color::Red)
                .add_modifier(Modifier::BOLD)
        };
        let text = vec![
            Line::styled(
                confirm.title(),
                Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
            )
            .alignment(Alignment::Center),
            Line::from(""),
            Line::from(confirm.body().to_string()).alignment(Alignment::Center),
            Line::from(""),
            Line::from(vec![
                Span::styled(" [Yes] ", yes_style),
                Span::raw("     "),
                Span::styled(" [No] ", no_style),
            ])
            .alignment(Alignment::Center),
        ];
        f.render_widget(
            Paragraph::new(text)
                .block(Block::default().style(Style::default().bg(theme.panel)))
                .alignment(Alignment::Center),
            popup,
        );
    }

    fn render_results(
        &self,
        f: &mut Frame,
        area: Rect,
        keybinding_overrides: &BTreeMap<String, String>,
    ) {
        let settings = all_settings();
        let rows = self.results(&settings);
        if rows.is_empty() {
            f.render_widget(
                Paragraph::new("No settings found")
                    .alignment(Alignment::Center)
                    .style(Style::default().fg(Color::DarkGray)),
                area,
            );
            return;
        }

        let mut y = area.y;
        let bottom = area.y + area.height;
        let mut last_group = None;
        let visible_rows = area.height as usize;

        let show_headers = self.current_category().is_none();
        let mut sim_y = 0;
        let mut sim_last_cat = None;
        let mut last_visible = self.scroll_offset;
        for (idx, (_, setting)) in rows.iter().enumerate().skip(self.scroll_offset) {
            if sim_y >= visible_rows {
                break;
            }
            if show_headers && Some(setting.category) != sim_last_cat {
                sim_last_cat = Some(setting.category);
                sim_y += 1;
                if sim_y >= visible_rows {
                    break;
                }
            }
            last_visible = idx;
            sim_y += 1;
        }

        let scroll_offset = if visible_rows == 0 {
            0
        } else if self.selected < self.scroll_offset {
            self.selected
        } else if self.selected > last_visible {
            let mut new_offset = self.selected;
            let mut fill = 1;
            let mut prev_cat = None;
            let mut check = self.selected;
            while check > 0 && fill < visible_rows {
                check -= 1;
                if show_headers {
                    if let Some((_, s)) = rows.get(check) {
                        if Some(s.category) != prev_cat {
                            prev_cat = Some(s.category);
                            fill += 1;
                        }
                    }
                }
                fill += 1;
                if fill < visible_rows {
                    new_offset = check;
                }
            }
            new_offset
        } else {
            self.scroll_offset
        };
        for (visible_idx, (_, setting)) in rows.iter().enumerate().skip(scroll_offset) {
            if y >= bottom {
                break;
            }
            if self.current_category().is_none() && Some(setting.category) != last_group {
                let _header = GroupHeader(setting.category.label());
                f.render_widget(
                    Paragraph::new(format!(" {} ", setting.category.label())).style(
                        Style::default()
                            .fg(setting.category.color())
                            .add_modifier(Modifier::BOLD),
                    ),
                    Rect::new(area.x, y, area.width, 1),
                );
                last_group = Some(setting.category);
                y += 1;
                if y >= bottom {
                    break;
                }
            }
            self.render_row(f, area, y, visible_idx, setting, keybinding_overrides);
            y += 1;
        }
    }

    fn render_row(
        &self,
        f: &mut Frame,
        area: Rect,
        y: u16,
        visible_idx: usize,
        setting: &Setting,
        keybinding_overrides: &BTreeMap<String, String>,
    ) {
        let selected = visible_idx == self.selected;
        let row_style = if selected {
            Style::default().bg(Color::DarkGray).fg(Color::White)
        } else {
            Style::default()
        };
        if self.current_category().is_some() {
            f.render_widget(
                Paragraph::new(" ").style(Style::default().bg(setting.category.color())),
                Rect::new(area.x, y, 1, 1),
            );
        }
        let prefix = match setting.setting_type {
            SettingType::Subsection => "▸",
            SettingType::Bool { enabled } => {
                if self
                    .bool_toggles
                    .get(setting.id)
                    .copied()
                    .unwrap_or(enabled)
                {
                    "✓"
                } else {
                    "○"
                }
            }
            SettingType::Keybind { .. } => "⌘",
            SettingType::Text | SettingType::Number | SettingType::Choice(_) => "•",
            SettingType::Theme(_) => "◉",
            SettingType::ToastPosition(_) => "◉",
            SettingType::DestructiveAction => "!",
        };
        let binding = match setting.setting_type {
            SettingType::Keybind {
                action_id,
                default_binding,
                reserved,
            } => {
                let current = keybinding_overrides
                    .get(action_id)
                    .map(String::as_str)
                    .unwrap_or(default_binding);
                if reserved {
                    format!(" [{current} reserved]")
                } else {
                    format!(" [{current}]")
                }
            }
            SettingType::Subsection
            | SettingType::Bool { .. }
            | SettingType::Text
            | SettingType::Number
            | SettingType::Choice(_)
            | SettingType::Theme(_)
            | SettingType::ToastPosition(_)
            | SettingType::DestructiveAction => String::new(),
        };
        let danger = match setting.danger {
            DangerLevel::Safe => "",
            DangerLevel::Warning => " [warning]",
            DangerLevel::Danger => " [danger]",
        };
        let x = if self.current_category().is_some() {
            area.x + 1
        } else {
            area.x
        };
        let width = if self.current_category().is_some() {
            area.width.saturating_sub(1)
        } else {
            area.width
        };
        f.render_widget(
            Paragraph::new(format!(
                " {prefix} {}{binding}{danger} — {}",
                setting.title, setting.description
            ))
            .style(row_style),
            Rect::new(x, y, width, 1),
        );
    }

    fn in_scope(&self, setting: &Setting) -> bool {
        match self.current_category() {
            Some(category) => {
                setting.category == category && setting.setting_type != SettingType::Subsection
            }
            None => true,
        }
    }
}

fn setting_matches(setting: &Setting, query: &str) -> bool {
    setting.title.to_lowercase().contains(query)
        || setting.description.to_lowercase().contains(query)
        || setting
            .keywords
            .iter()
            .any(|keyword| keyword.contains(query))
}

#[rustfmt::skip]
pub fn all_settings() -> Vec<Setting> {
    use DangerLevel as D;
    use SettingCategory as C;
    use SettingType as T;

    vec![
        subsection("theme", "Theme", "Colors and visual style", C::Theme),
        subsection("ui_layout", "UI Layout", "Panels and status widgets", C::UiLayout),
        subsection("chat_behavior", "Chat Behavior", "Chat input and output defaults", C::ChatBehavior),
        setting("theme_system", "System", "Use terminal default colors", C::Theme, T::Theme("system"), &["theme", "default"], D::Safe),
        setting("theme_gruvbox_dark_low_contrast", "Gruvbox Dark Low Contrast", "Warm muted dark palette", C::Theme, T::Theme("gruvbox-dark-low-contrast"), &["theme", "color", "gruvbox", "low", "contrast"], D::Safe),
        setting("theme_gruvbox_dark_high_contrast", "Gruvbox Dark High Contrast", "Warm vivid dark palette", C::Theme, T::Theme("gruvbox-dark-high-contrast"), &["theme", "color", "gruvbox", "high", "contrast"], D::Safe),
        setting("theme_nord", "Nord", "Arctic blue palette", C::Theme, T::Theme("nord"), &["theme", "color"], D::Safe),
        setting("theme_dracula", "Dracula", "Purple dark palette", C::Theme, T::Theme("dracula"), &["theme", "color"], D::Safe),
        setting("theme_github", "GitHub", "GitHub dark palette", C::Theme, T::Theme("github"), &["theme", "color"], D::Safe),
        setting("theme_kanagawa", "Kanagawa", "Japanese ink palette", C::Theme, T::Theme("kanagawa"), &["theme", "color"], D::Safe),
        setting("theme_catppuccin", "Catppuccin", "Pastel dark palette", C::Theme, T::Theme("catppuccin"), &["theme", "color"], D::Safe),
        setting("theme_material", "Material", "Material dark palette", C::Theme, T::Theme("material"), &["theme", "color"], D::Safe),
        setting("theme_matrix", "Matrix", "Green terminal palette", C::Theme, T::Theme("matrix"), &["theme", "color"], D::Safe),
        setting("theme_monokai", "Monokai", "Monokai dark palette", C::Theme, T::Theme("monokai"), &["theme", "color"], D::Safe),
        setting("theme_zenburn", "Zenburn", "Low-contrast green palette", C::Theme, T::Theme("zenburn"), &["theme", "color"], D::Safe),
        setting("theme_solarized", "Solarized", "Solarized dark palette", C::Theme, T::Theme("solarized"), &["theme", "color"], D::Safe),
        setting("theme_tokyo_night", "Tokyo Night", "Tokyo Night palette", C::Theme, T::Theme("tokyo-night"), &["theme", "color"], D::Safe),
        setting("theme_opencode", "OpenCode", "OpenCode-inspired palette", C::Theme, T::Theme("opencode"), &["theme", "color"], D::Safe),
        setting("toast_top_right", "Toast: Top Right", "Show toast notifications at the top right", C::UiLayout, T::ToastPosition(crate::config::ToastPosition::TopRight), &["toast", "notification", "position"], D::Safe),
        setting("toast_top_center", "Toast: Top Center", "Show toast notifications at the top center", C::UiLayout, T::ToastPosition(crate::config::ToastPosition::TopCenter), &["toast", "notification", "position"], D::Safe),
        setting("toast_top_left", "Toast: Top Left", "Show toast notifications at the top left", C::UiLayout, T::ToastPosition(crate::config::ToastPosition::TopLeft), &["toast", "notification", "position"], D::Safe),
        setting("toast_center", "Toast: Center", "Show toast notifications in the center", C::UiLayout, T::ToastPosition(crate::config::ToastPosition::Center), &["toast", "notification", "position"], D::Safe),
        setting("toast_off", "Toast: Off", "Disable toast notifications", C::UiLayout, T::ToastPosition(crate::config::ToastPosition::Off), &["toast", "notification", "position"], D::Safe),
        setting("web_search", "Web Search", "Enable web search for prompts", C::ChatBehavior, T::Bool { enabled: false }, &["search", "prompt"], D::Safe),
        setting("collapse_thinking", "Collapse Thinking", "Fold assistant thinking by default", C::ChatBehavior, T::Bool { enabled: true }, &["reasoning", "fold"], D::Safe),
    ]
}

fn subsection(
    id: &'static str,
    title: &'static str,
    description: &'static str,
    category: SettingCategory,
) -> Setting {
    setting(
        id,
        title,
        description,
        category,
        SettingType::Subsection,
        &[],
        DangerLevel::Safe,
    )
}

fn setting(
    id: &'static str,
    title: &'static str,
    description: &'static str,
    category: SettingCategory,
    setting_type: SettingType,
    keywords: &'static [&'static str],
    danger: DangerLevel,
) -> Setting {
    Setting {
        id,
        title,
        description,
        category,
        setting_type,
        keywords,
        danger,
    }
}

fn keybind(
    action_id: &'static str,
    title: &'static str,
    description: &'static str,
    default_binding: &'static str,
    reserved: bool,
    keywords: &'static [&'static str],
) -> Setting {
    setting(
        action_id,
        title,
        description,
        SettingCategory::Keybind,
        SettingType::Keybind {
            action_id,
            default_binding,
            reserved,
        },
        keywords,
        if reserved {
            DangerLevel::Warning
        } else {
            DangerLevel::Safe
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_settings_has_expected_stub_and_setting_counts() {
        let settings = all_settings();
        let subsections = settings
            .iter()
            .filter(|setting| setting.setting_type == SettingType::Subsection)
            .count();

        assert_eq!(settings.len(), 25);
        assert_eq!(subsections, 3);
        assert_eq!(SettingCategory::ALL.len(), 12);
    }

    #[test]
    fn root_results_show_every_setting_when_query_empty() {
        let settings = all_settings();
        let panel = SettingsPanelState::new();

        let results = panel.results(&settings);

        assert_eq!(results.len(), 25);
    }

    #[test]
    fn theme_subsection_lists_selectable_real_themes() {
        let settings = all_settings();
        let mut panel = SettingsPanelState::new();
        panel.depth_stack.push(SettingCategory::Theme);

        let results = panel.results(&settings);

        assert!(results.iter().any(|(_, setting)| {
            matches!(setting.setting_type, SettingType::Theme("opencode"))
        }));
        assert!(results
            .iter()
            .all(|(_, setting)| { matches!(setting.setting_type, SettingType::Theme(_)) }));
    }

    #[test]
    fn search_filters_title_description_and_keywords() {
        let settings = all_settings();
        let mut panel = SettingsPanelState::new();
        for c in "gruvbox".chars() {
            panel.insert_char(c);
        }

        let results = panel.results(&settings);

        assert!(results
            .iter()
            .any(|(_, setting)| { setting.id == "theme_gruvbox_dark_low_contrast" }));
        assert!(results
            .iter()
            .any(|(_, setting)| { setting.id == "theme_gruvbox_dark_high_contrast" }));
        assert!(results
            .iter()
            .all(|(_, setting)| setting_matches(setting, "gruvbox")));
    }

    #[test]
    fn enter_subsection_filters_to_category_and_excludes_subsection_rows() {
        let settings = all_settings();
        let mut panel = SettingsPanelState::new();

        assert_eq!(panel.enter(&settings), EnterResult::EnteredSubsection);
        let results = panel.results(&settings);

        assert_eq!(panel.current_category(), Some(SettingCategory::Theme));
        assert_eq!(results.len(), 15);
        assert!(results.iter().all(|(_, setting)| {
            setting.category == SettingCategory::Theme
                && setting.setting_type != SettingType::Subsection
        }));
    }

    #[test]
    fn escape_pops_depth_before_closing() {
        let settings = all_settings();
        let mut panel = SettingsPanelState::new();
        assert_eq!(panel.enter(&settings), EnterResult::EnteredSubsection);

        assert!(!panel.esc());
        assert_eq!(panel.current_category(), None);
        assert!(panel.esc());
    }
}
