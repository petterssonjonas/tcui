use ratatui::{prelude::*, widgets::*, Frame};

use crate::tui::components::{centered_rect, SearchField};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SettingCategory {
    Commands,
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
    Notifications,
    Terminal,
    WebSearch,
    #[cfg(feature = "memory")]
    Memory,
}

impl SettingCategory {
    pub fn label(self) -> &'static str {
        match self {
            Self::Commands => "Commands",
            Self::Provider => "Connections",
            Self::Model => "Models",
            Self::Local => "Local inference",
            Self::Theme => "Appearance",
            Self::Keybind => "Keyboard",
            Self::Mcp => "MCP",
            Self::Obsidian => "Files & vaults",
            Self::UiLayout => "Layout",
            Self::ChatBehavior => "Chat",
            Self::Privacy => "Privacy",
            Self::Notifications => "Notifications",
            Self::Terminal => "Terminal",
            Self::WebSearch => "Web search",
            #[cfg(feature = "memory")]
            Self::Memory => "Memory",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DangerLevel {
    Safe,
    Warning,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SettingType {
    Command(&'static str),
    Text,
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
    RunCommand(&'static str),
    NavigateSetting(&'static str),
}

#[derive(Debug, Clone, Copy)]
struct PanelLayout {
    outer: Rect,
    settings: Rect,
    description: Option<Rect>,
}

/// Search-only navigator. Configuration editing belongs to the Settings workspace.
#[derive(Debug, Clone, Default)]
pub struct SettingsPanelState {
    pub search: SearchField,
    pub selected: usize,
    pub scroll_offset: usize,
    show_descriptions: bool,
}

impl SettingsPanelState {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn query(&self) -> &str {
        self.search.query()
    }
    pub fn toggle_descriptions(&mut self) {
        self.show_descriptions = !self.show_descriptions;
    }
    pub fn esc(&mut self) -> bool {
        true
    }

    pub fn insert_char(&mut self, character: char) {
        self.search.insert(&character.to_string());
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
    }
    pub fn move_down(&mut self, settings: &[Setting]) {
        self.selected = self
            .selected
            .saturating_add(1)
            .min(self.results(settings).len().saturating_sub(1));
    }

    pub fn results<'a>(&self, settings: &'a [Setting]) -> Vec<(usize, &'a Setting)> {
        let query = self.query().trim().to_lowercase();
        settings
            .iter()
            .enumerate()
            .filter(|(_, setting)| {
                if query.is_empty() {
                    setting.id == "settings_tab"
                } else {
                    setting_matches(setting, &query)
                }
            })
            .collect()
    }

    pub fn selected_setting<'a>(&self, settings: &'a [Setting]) -> Option<&'a Setting> {
        self.results(settings)
            .get(self.selected)
            .map(|(_, setting)| *setting)
    }

    pub fn select(&mut self, index: usize, settings: &[Setting]) {
        self.selected = index.min(self.results(settings).len().saturating_sub(1));
    }

    pub fn enter(&mut self, settings: &[Setting]) -> EnterResult {
        match self.selected_setting(settings) {
            Some(Setting {
                setting_type: SettingType::Command(command),
                ..
            }) => EnterResult::RunCommand(command),
            Some(setting) => EnterResult::NavigateSetting(setting.id),
            None => EnterResult::Nothing,
        }
    }

    fn panel_layout(&self, area: Rect) -> PanelLayout {
        let compact = centered_rect(42, 60, area);
        let settings_width = compact.width.max(33).min(area.width);
        let description_width = if self.show_descriptions {
            area.width * 28 / 100
        } else {
            0
        };
        let total_width = settings_width
            .saturating_add(if description_width > 0 {
                description_width + 1
            } else {
                0
            })
            .min(area.width);
        let outer = Rect::new(
            area.x + area.width.saturating_sub(total_width) / 2,
            compact.y,
            total_width,
            compact.height,
        );
        let settings = Rect::new(
            outer.x,
            outer.y,
            settings_width.min(outer.width),
            outer.height,
        );
        let description_x = settings.right().saturating_add(1);
        let description = (self.show_descriptions && description_x < outer.right()).then(|| {
            Rect::new(
                description_x,
                outer.y,
                outer.right() - description_x,
                outer.height,
            )
        });
        PanelLayout {
            outer,
            settings,
            description,
        }
    }

    pub fn popup_area(&self, area: Rect) -> Rect {
        self.panel_layout(area).outer
    }

    fn list_area(&self, area: Rect) -> Rect {
        let inner = self.panel_layout(area).settings.inner(Margin {
            horizontal: 1,
            vertical: 1,
        });
        Rect::new(
            inner.x,
            inner.y.saturating_add(2).min(inner.bottom()),
            inner.width,
            inner.height.saturating_sub(2),
        )
    }

    fn offset(&self, height: u16) -> usize {
        if height == 0 || self.selected < self.scroll_offset {
            self.selected
        } else if self.selected >= self.scroll_offset.saturating_add(height as usize) {
            self.selected + 1 - height as usize
        } else {
            self.scroll_offset
        }
    }

    pub fn visible_item_areas(&self, area: Rect) -> Vec<(Rect, usize)> {
        let list = self.list_area(area);
        let settings = all_settings();
        let count = self.results(&settings).len();
        let offset = self.offset(list.height);
        (offset..count)
            .take(list.height as usize)
            .enumerate()
            .map(|(row, index)| (Rect::new(list.x, list.y + row as u16, list.width, 1), index))
            .collect()
    }

    pub fn render(&self, frame: &mut Frame, area: Rect) {
        let theme = crate::theme::active_theme();
        let layout = self.panel_layout(area);
        let style = Style::default().fg(theme.foreground).bg(theme.panel);
        frame.render_widget(Clear, layout.outer);
        frame.render_widget(Block::default().style(style), layout.settings);
        let hint = if self.show_descriptions {
            "[Tab] Hide"
        } else {
            "[Tab] Details"
        };
        frame.render_widget(
            Paragraph::new(format!(" Settings search {hint} "))
                .style(style.fg(theme.accent).add_modifier(Modifier::BOLD)),
            Rect::new(
                layout.settings.x.saturating_add(1),
                layout.settings.y,
                layout.settings.width.saturating_sub(2),
                1.min(layout.settings.height),
            ),
        );
        let inner = layout.settings.inner(Margin {
            horizontal: 1,
            vertical: 1,
        });
        frame.render_widget(
            Paragraph::new(format!("> {}", self.query())).style(style.fg(theme.warning)),
            Rect::new(inner.x, inner.y, inner.width, 1.min(inner.height)),
        );
        let catalog = all_settings();
        let results = self.results(&catalog);
        let list = self.list_area(area);
        if results.is_empty() {
            frame.render_widget(
                Paragraph::new("No matching settings or actions").style(style.fg(theme.muted)),
                list,
            );
        }
        for (row, (index, (_, setting))) in results
            .iter()
            .enumerate()
            .skip(self.offset(list.height))
            .take(list.height as usize)
            .enumerate()
        {
            let style = if index == self.selected {
                style.fg(theme.accent).add_modifier(Modifier::REVERSED)
            } else {
                style
            };
            let marker = if matches!(setting.setting_type, SettingType::Command(_)) {
                "›"
            } else {
                "↗"
            };
            let label = if setting.id == "settings_tab" && self.query().trim().is_empty() {
                format!(" {marker} {}", setting.title)
            } else {
                format!(" {marker} {} · {}", setting.title, setting.category.label())
            };
            frame.render_widget(
                Paragraph::new(label).style(style),
                Rect::new(list.x, list.y + row as u16, list.width, 1),
            );
        }
        if let Some(description) = layout.description {
            frame.render_widget(Block::default().style(style), description);
            let inner = description.inner(Margin {
                horizontal: 1,
                vertical: 1,
            });
            let copy = self.selected_setting(&catalog).map(|setting| format!("Description\n\n{}\n\n{}\n\nEnter opens the dedicated settings page or runs this action.", setting.title, setting.description)).unwrap_or_default();
            frame.render_widget(
                Paragraph::new(copy).wrap(Wrap { trim: true }).style(style),
                inner,
            );
        }
    }
}

fn setting_matches(setting: &Setting, query: &str) -> bool {
    setting.id.to_lowercase().contains(query)
        || setting.title.to_lowercase().contains(query)
        || setting.description.to_lowercase().contains(query)
        || setting.category.label().to_lowercase().contains(query)
        || setting
            .keywords
            .iter()
            .any(|keyword| keyword.contains(query))
}

pub fn command_action(id: &str) -> Option<crate::app::Action> {
    use crate::app::Action;
    match id {
        "open_settings" | "open_settings_panel" => Some(Action::OpenSettingsPanel),
        "toggle_settings" => Some(Action::ToggleSettings),
        "connect_provider" => Some(Action::OpenProviders),
        "toggle_session_list" => Some(Action::ToggleSessionList),
        "toggle_web_search" => Some(Action::ToggleWebSearch),
        "show_local_search" => Some(Action::ShowLocalSearch(String::new())),
        "open_terminal" => Some(Action::OpenTerminal),
        "new_chat" => Some(Action::NewChat),
        "close_chat" => Some(Action::CloseChat),
        "toggle_sidebar" => Some(Action::ToggleSidebar),
        "toggle_artifact_sidebar" => Some(Action::ToggleArtifactSidebar),
        "refresh_models" => Some(Action::RefreshModels),
        "open_providers" => Some(Action::OpenProviders),
        "open_credential_vault" => Some(Action::OpenCredentialVault),
        "open_security" => Some(Action::OpenSecurity),
        "unlock_application" => Some(Action::UnlockApplication),
        "lock_application" => Some(Action::LockApplication),
        "search_api_keys" => Some(Action::OpenSearchCredentials),
        "export_conversation" => Some(Action::ExportConversation),
        "show_skills" => Some(Action::ShowSkillsPopup),
        "show_mcp" => Some(Action::ShowMcpPopup),
        "show_help" => Some(Action::ShowHelp),
        "show_keybinds" => Some(Action::ShowKeybinds),
        "focus_input" => Some(Action::FocusInput),
        "quit" => Some(Action::ShowQuitConfirm),
        "manage_vault_profiles" => Some(Action::OpenVaultProfileManager),
        _ => None,
    }
}

pub fn all_settings() -> Vec<Setting> {
    use DangerLevel as D;
    use SettingCategory as C;
    use SettingType as T;
    let mut settings = vec![
        setting(
            "settings_tab",
            "Settings tab",
            "Open categorized settings. Each change is validated, applied and saved individually.",
            C::Commands,
            T::Command("open_settings"),
            &["preferences", "configuration"],
            D::Safe,
        ),
        setting(
            "application_protection",
            "Application protection",
            "Choose entry password and encryption independently for credentials, chats, memories and the internal vault. Unchecked data remains plaintext.",
            C::Commands,
            T::Command("open_security"),
            &["security", "privacy", "password", "encryption", "lock"],
            D::Safe,
        ),
        setting(
            "command_new_chat",
            "New Chat",
            "Start a new conversation.",
            C::Commands,
            T::Command("new_chat"),
            &["conversation"],
            D::Safe,
        ),
        setting(
            "command_open_terminal",
            "Open terminal",
            "Open a persistent terminal session on the TCUI execution host.",
            C::Commands,
            T::Command("open_terminal"),
            &["terminal", "shell"],
            D::Safe,
        ),
        setting(
            "command_close_chat",
            "Close Chat",
            "Close the active conversation tab.",
            C::Commands,
            T::Command("close_chat"),
            &["tab"],
            D::Safe,
        ),
        setting(
            "command_toggle_sidebar",
            "Toggle Sidebar",
            "Show or hide the conversation sidebar.",
            C::Commands,
            T::Command("toggle_sidebar"),
            &["panel"],
            D::Safe,
        ),
        setting(
            "command_toggle_artifact_sidebar",
            "Toggle Chat files sidebar",
            "Show or hide the Chat files sidebar.",
            C::Commands,
            T::Command("toggle_artifact_sidebar"),
            &["files"],
            D::Safe,
        ),
        setting(
            "command_refresh_models",
            "Refresh Models",
            "Refresh available provider models.",
            C::Commands,
            T::Command("refresh_models"),
            &["provider"],
            D::Safe,
        ),
        setting(
            "command_toggle_session_list",
            "Toggle Session List",
            "Show or hide the saved conversation list.",
            C::Commands,
            T::Command("toggle_session_list"),
            &["session", "history"],
            D::Safe,
        ),
        setting(
            "command_toggle_web_search",
            "Toggle Web Search",
            "Enable or disable web search for subsequent prompts.",
            C::Commands,
            T::Command("toggle_web_search"),
            &["web", "search"],
            D::Safe,
        ),
        setting(
            "command_show_local_search",
            "Search Markdown Vault",
            "Open the local Markdown Vault search.",
            C::Commands,
            T::Command("show_local_search"),
            &["vault", "search"],
            D::Safe,
        ),
        setting(
            "command_export_conversation",
            "Export Conversation",
            "Export the active conversation.",
            C::Commands,
            T::Command("export_conversation"),
            &["save"],
            D::Safe,
        ),
        setting(
            "command_show_skills",
            "Show Skills",
            "Browse available agent skills.",
            C::Commands,
            T::Command("show_skills"),
            &["agent"],
            D::Safe,
        ),
        setting(
            "command_show_mcp",
            "Show MCP",
            "Browse configured MCP servers and tools.",
            C::Commands,
            T::Command("show_mcp"),
            &["tools"],
            D::Safe,
        ),
        setting(
            "command_show_help",
            "Help",
            "Open keyboard and workflow help.",
            C::Commands,
            T::Command("show_help"),
            &["help"],
            D::Safe,
        ),
        setting(
            "command_show_keybinds",
            "Keybindings",
            "Show current keyboard shortcuts.",
            C::Commands,
            T::Command("show_keybinds"),
            &["keys"],
            D::Safe,
        ),
        setting(
            "command_focus_input",
            "Focus Input",
            "Return focus to the chat composer.",
            C::Commands,
            T::Command("focus_input"),
            &["prompt"],
            D::Safe,
        ),
        setting(
            "command_quit",
            "Quit",
            "Close TCUI, confirming when configured.",
            C::Commands,
            T::Command("quit"),
            &["exit"],
            D::Warning,
        ),
    ];
    settings.extend(crate::ui::settings_workspace::navigator_settings());
    settings
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

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::{backend::TestBackend, Terminal};

    #[test]
    fn empty_navigator_only_offers_settings_tab() {
        let settings = all_settings();
        let mut state = SettingsPanelState::new();
        assert_eq!(state.results(&settings).len(), 1);
        assert_eq!(
            state.selected_setting(&settings).expect("entry").title,
            "Settings tab"
        );
        assert_eq!(
            state.enter(&settings),
            EnterResult::RunCommand("open_settings")
        );
    }

    #[test]
    fn search_navigates_setting_without_mutation() {
        let settings = all_settings();
        let mut state = SettingsPanelState::new();
        for character in "collapse thinking".chars() {
            state.insert_char(character);
        }
        assert_eq!(
            state.enter(&settings),
            EnterResult::NavigateSetting("collapse_thinking")
        );
    }

    #[test]
    fn search_finds_nested_configuration_and_commands() {
        let settings = all_settings();
        for (query, id) in [
            ("connect timeout", "local_inference.connect_timeout_ms"),
            ("focus input", "command_focus_input"),
            ("terminal placement", "tui.terminal_placement"),
        ] {
            let mut state = SettingsPanelState::new();
            for character in query.chars() {
                state.insert_char(character);
            }
            assert!(
                state
                    .results(&settings)
                    .iter()
                    .any(|(_, setting)| setting.id == id),
                "{query}"
            );
        }
    }

    #[test]
    fn navigator_keeps_compact_layout_and_optional_details() {
        let mut state = SettingsPanelState::new();
        assert!(state
            .panel_layout(Rect::new(0, 0, 100, 40))
            .description
            .is_none());
        let mut terminal = Terminal::new(TestBackend::new(64, 16)).expect("terminal");
        terminal
            .draw(|frame| state.render(frame, frame.area()))
            .expect("render");
        let rendered = terminal.backend().to_string();
        assert!(rendered.contains("Settings search"));
        assert!(!rendered.contains("Settings tab · Commands"));
        state.toggle_descriptions();
        assert!(state
            .panel_layout(Rect::new(0, 0, 100, 40))
            .description
            .is_some());
    }

    #[test]
    fn mouse_rows_match_search_result_geometry() {
        let mut state = SettingsPanelState::new();
        for character in "local".chars() {
            state.insert_char(character);
        }
        let rows = state.visible_item_areas(Rect::new(0, 0, 64, 16));
        assert!(!rows.is_empty());
        assert_eq!(rows[0].1, 0);
        assert!(rows
            .windows(2)
            .all(|rows| rows[0].0.bottom() == rows[1].0.y));
    }
}
