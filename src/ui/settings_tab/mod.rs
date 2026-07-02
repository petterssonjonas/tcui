pub mod state;
mod focus;
mod general;
mod keybindings;
mod local;
mod mcp;
mod models;
mod providers;
mod util;

pub use state::{
    EditProvidersFocus, EditProvidersHitAreas, EditProvidersPopupState, EditableProvider,
    GeneralDropdown, GeneralFocus, GeneralHitAreas, LocalFocus, LocalHitAreas, McpHitAreas,
    ModelInfo, ModelsTabFocus, ModelsTabHitAreas, PresetKeyPopupState, ProviderEntry,
    ProviderFormFocus, ProviderFormHitAreas, ProviderFormState, ProvidersAction,
    ProvidersDropdown, ProvidersTabFocus, ProvidersTabHitAreas,
    SettingsPopupInit,
};
#[allow(unused_imports)]
pub use state::ProvidersFocus;

use crate::config::app_config::{HeadingDownscale, LocalServerType, MarkdownMode, TextAlignment};
use crate::config::McpServerConfig;
pub(crate) use state::{
    ALIGNMENT_OPTIONS, BACKEND_TYPE_OPTIONS, KITTY_HEADING_SIZE_OPTIONS, PRESET_PROVIDER_NAMES,
    SEARCH_KEY_PROVIDERS,
};
use ratatui::{
    layout::{Position, Rect},
    prelude::*,
    widgets::*,
    Frame,
};

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum SettingsTab {
    General,
    Keybindings,
    Providers,
    Models,
    Local,
    Mcp,
}

#[derive(Debug, Clone)]
pub struct SettingsPopup {
    pub default_provider: String,
    pub default_model: String,
    pub small_model: String,
    pub active_tab: SettingsTab,
    pub use_env_keys: bool,
    pub saved_keys: Vec<(String, String)>,
    pub theme: String,
    pub user_alignment: TextAlignment,
    pub ai_alignment: TextAlignment,
    pub markdown_mode: MarkdownMode,
    pub artifact_save_dir: String,
    pub general_focus: GeneralFocus,
    pub general_hit_areas: GeneralHitAreas,
    pub general_dropdown_open: Option<GeneralDropdown>,
    pub available_models: Vec<ModelInfo>,
    pub db_providers: Vec<ProviderEntry>,
    pub show_selector: bool,
    pub show_chat_scrollbar: bool,
    pub collapse_thinking: bool,
    pub kitty_enhanced_text: bool,
    pub kitty_heading_downscale: HeadingDownscale,
    pub web_search_enabled: bool,
    pub quit_confirmation: bool,
    pub local_enabled: bool,
    pub local_host: String,
    pub local_port: String,
    pub local_server_type: LocalServerType,
    pub local_selected_model: String,
    pub local_model_directory: String,
    pub local_health_interval_seconds: String,
    pub local_connect_timeout_ms: String,
    pub local_request_timeout_ms: String,
    pub local_api_token_env: String,
    pub detected_local_server: Option<LocalServerType>,
    pub local_focus: LocalFocus,
    pub local_hit_areas: LocalHitAreas,
    pub mcp_servers: Vec<McpServerConfig>,
    pub mcp_focus: usize,
    pub mcp_hit_areas: McpHitAreas,
    pub providers_tab_list: Vec<EditableProvider>,
    pub providers_tab_focus: ProvidersTabFocus,
    pub providers_dropdown_open: Option<ProvidersDropdown>,
    pub dropdown_scroll_offset: usize,
    pub providers_tab_hit_areas: ProvidersTabHitAreas,
    pub models_provider: String,
    pub models_available_models: Vec<ModelInfo>,
    pub models_tab_focus: ModelsTabFocus,
    pub models_dropdown_open: bool,
    pub models_dropdown_scroll_offset: usize,
    pub models_tab_hit_areas: ModelsTabHitAreas,
    pub add_provider_popup: Option<ProviderFormState>,
    pub edit_providers_popup: Option<EditProvidersPopupState>,
    pub edit_provider_popup: Option<ProviderFormState>,

    pub preset_key_popup: Option<PresetKeyPopupState>,
    pub disabled_providers: std::collections::HashSet<String>,
    pub disabled_models: std::collections::HashSet<String>,
}

impl SettingsPopup {
pub fn new(init: SettingsPopupInit) -> Self {
    Self {
        default_provider: init.default_provider,
        default_model: init.default_model,
        small_model: init.small_model,
        active_tab: SettingsTab::General,
        general_focus: GeneralFocus::Theme,
        use_env_keys: init.use_env_keys,
        saved_keys: init.saved_keys,
        theme: init.theme,
        user_alignment: init.user_alignment,
        ai_alignment: init.ai_alignment,
        markdown_mode: init.markdown_mode,
        artifact_save_dir: init.artifact_save_dir,
        general_hit_areas: GeneralHitAreas::default(),
        general_dropdown_open: None,
        available_models: init.available_models,
        db_providers: init.db_providers,
        show_selector: init.show_selector,
        show_chat_scrollbar: init.show_chat_scrollbar,
        collapse_thinking: init.collapse_thinking,
        kitty_enhanced_text: init.kitty_enhanced_text,
        kitty_heading_downscale: init.kitty_heading_downscale,
        web_search_enabled: init.web_search_enabled,
        quit_confirmation: init.quit_confirmation,
        local_enabled: init.local_enabled,
        local_host: init.local_host,
        local_port: init.local_port,
        local_server_type: init.local_server_type,
        local_selected_model: init.local_selected_model,
        local_model_directory: init.local_model_directory,
        local_health_interval_seconds: init.local_health_interval_seconds,
        local_connect_timeout_ms: init.local_connect_timeout_ms,
        local_request_timeout_ms: init.local_request_timeout_ms,
        local_api_token_env: init.local_api_token_env,
        detected_local_server: init.detected_local_server,
        local_focus: LocalFocus::Enabled,
        local_hit_areas: LocalHitAreas::default(),
        mcp_servers: init.mcp_servers,
        mcp_focus: 0,
        mcp_hit_areas: McpHitAreas::default(),
        providers_tab_list: init.providers_tab_list,
        providers_tab_focus: ProvidersTabFocus::DefaultProvider,
        providers_dropdown_open: None,
        dropdown_scroll_offset: 0,
        providers_tab_hit_areas: ProvidersTabHitAreas::default(),
        models_provider: init.models_provider,
        models_available_models: init.models_available_models,
        models_tab_focus: ModelsTabFocus::Provider,
        models_dropdown_open: false,
        models_dropdown_scroll_offset: 0,
        models_tab_hit_areas: ModelsTabHitAreas::default(),
        add_provider_popup: None,
        edit_providers_popup: None,
        edit_provider_popup: None,

        preset_key_popup: None,
        disabled_providers: std::collections::HashSet::new(),
        disabled_models: std::collections::HashSet::new(),
    }
}

pub fn render(&mut self, f: &mut Frame) {
    let area = f.area();
    let popup_area = Self::centered_rect(65, 75, area);

    f.render_widget(Clear, area);
    f.render_widget(
        Block::default().style(Style::default().bg(Color::Black)),
        area,
    );

    let block = Block::default()
        .title(" Settings ")
        .title_style(
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        )
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::DarkGray))
        .style(Style::default().bg(Color::Black));

    let inner = block.inner(popup_area);
    f.render_widget(Clear, popup_area);
    f.render_widget(block, popup_area);

    let tab_layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(3), Constraint::Min(0)])
        .split(inner);

    let tabs = [
        "General",
        "Keybindings",
        "Providers",
        "Models",
        "Local",
        "MCP",
    ];
    let tab_titles: Vec<Line> = tabs
        .iter()
        .enumerate()
        .map(|(i, t)| {
            let style = if self.active_tab == Self::tab_from_index(i) {
                Style::default()
                    .fg(Color::Black)
                    .bg(Color::Cyan)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::Gray)
            };
            Line::styled((*t).to_string(), style)
        })
        .collect();

    let tabs_widget = Tabs::new(tab_titles)
        .select(self.tab_index())
        .divider(Span::styled("│", Style::default().fg(Color::DarkGray)))
        .padding("", "")
        .style(Style::default().bg(Color::Black));
    f.render_widget(tabs_widget, tab_layout[0]);

    match self.active_tab {
        SettingsTab::General => self.render_general(f, tab_layout[1]),
        SettingsTab::Keybindings => self.render_keybindings(f, tab_layout[1]),
        SettingsTab::Providers => self.render_providers(f, tab_layout[1]),
        SettingsTab::Models => self.render_models(f, tab_layout[1]),
        SettingsTab::Local => self.render_local(f, tab_layout[1]),
        SettingsTab::Mcp => self.render_mcp(f, tab_layout[1]),
    }

    if let Some(edit_popup) = &mut self.edit_providers_popup {
        Self::render_edit_providers_popup(f, popup_area, &self.providers_tab_list, edit_popup);
    }

    if let Some(form) = &mut self.add_provider_popup {
        Self::render_provider_form_popup(f, popup_area, form);
    }

    if let Some(form) = &mut self.edit_provider_popup {
        Self::render_provider_form_popup(f, popup_area, form);
    }
}

pub fn provider_popup_active(&self) -> bool {
    self.add_provider_popup.is_some()
        || self.edit_providers_popup.is_some()
        || self.edit_provider_popup.is_some()
        || self.preset_key_popup.is_some()
}

pub fn close_active_provider_popup(&mut self) -> bool {
    self.preset_key_popup.take().is_some()
        || self.edit_provider_popup.take().is_some()
        || self.edit_providers_popup.take().is_some()
        || self.add_provider_popup.take().is_some()
}

pub(super) fn tab_index(&self) -> usize {
    match self.active_tab {
        SettingsTab::General => 0,
        SettingsTab::Keybindings => 1,
        SettingsTab::Providers => 2,
        SettingsTab::Models => 3,
        SettingsTab::Local => 4,
        SettingsTab::Mcp => 5,
    }
}

pub(super) fn tab_from_index(i: usize) -> SettingsTab {
    match i {
        0 => SettingsTab::General,
        1 => SettingsTab::Keybindings,
        2 => SettingsTab::Providers,
        3 => SettingsTab::Models,
        4 => SettingsTab::Local,
        5 => SettingsTab::Mcp,
        _ => SettingsTab::General,
    }
}

pub fn tab_hit_areas(&self, area: Rect) -> Vec<Rect> {
    let inner = Rect::new(area.x + 1, area.y + 1, area.width - 2, area.height - 2);
    let tab_row = Rect::new(inner.x, inner.y, inner.width, 3);
    let tabs = [
        "General",
        "Keybindings",
        "Providers",
        "Models",
        "Local",
        "MCP",
    ];
    let mut areas = Vec::new();
    let mut current_x = tab_row.x;
    for t in tabs {
        let width = t.len() as u16;
        areas.push(Rect::new(current_x, tab_row.y, width, tab_row.height));
        current_x += width + 1;
    }
    areas
}

pub fn popup_area(area: Rect) -> Rect {
    Self::centered_rect(65, 75, area)
}

}

#[cfg(test)]
mod tests;
