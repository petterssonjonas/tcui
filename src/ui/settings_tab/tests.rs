use super::*;
use ratatui::{backend::TestBackend, Terminal};

fn popup_with_mcps(count: usize) -> SettingsPopup {
    SettingsPopup::new(SettingsPopupInit {
        default_provider: String::new(),
        default_model: String::new(),
        small_model: String::new(),
        use_env_keys: false,
        saved_keys: vec![],
        theme: "system".to_string(),
        user_alignment: TextAlignment::Left,
        ai_alignment: TextAlignment::Left,
        markdown_mode: MarkdownMode::Full,
        artifact_save_dir: String::new(),
        available_models: vec![],
        db_providers: vec![],
        show_selector: true,
        show_chat_scrollbar: true,
        collapse_thinking: true,
        kitty_enhanced_text: false,
        kitty_heading_downscale: HeadingDownscale::None,
        web_search_enabled: false,
        quit_confirmation: true,
        local_enabled: false,
        local_host: String::new(),
        local_port: String::new(),
        local_server_type: LocalServerType::Auto,
        local_selected_model: String::new(),
        local_model_directory: String::new(),
        local_health_interval_seconds: String::new(),
        local_connect_timeout_ms: String::new(),
        local_request_timeout_ms: String::new(),
        local_api_token_env: String::new(),
        detected_local_server: None,
        providers_tab_list: vec![],
        models_provider: String::new(),
        models_available_models: vec![],
        mcp_servers: (0..count)
            .map(|idx| McpServerConfig {
                name: format!("server-{idx}"),
                enabled: idx % 2 == 0,
                ..McpServerConfig::default()
            })
            .collect(),
    })
}

#[test]
fn mcp_settings_render_and_hit_rows_fit_80_by_24() {
    // Given
    let mut popup = popup_with_mcps(20);
    popup.active_tab = SettingsTab::Mcp;
    popup.mcp_focus = 15;
    let mut terminal = Terminal::new(TestBackend::new(80, 24)).expect("test terminal");

    // When
    terminal
        .draw(|frame| popup.render(frame))
        .expect("render MCP settings");

    // Then
    assert!(popup
        .mcp_hit_areas
        .rows
        .iter()
        .any(|(idx, area)| *idx == 15 && area.height == 1));
    assert!(popup
        .mcp_hit_areas
        .rows
        .iter()
        .all(|(_, area)| area.right() <= 80 && area.bottom() <= 24));
    let rendered: String = terminal
        .backend()
        .buffer()
        .content
        .iter()
        .map(|cell| cell.symbol())
        .collect();
    assert!(rendered.contains("MCP"));
    assert!(rendered.contains("server-15"));
}

#[test]
fn tab_hit_areas_include_models_before_local_and_mcp() {
    // Given
    let popup = popup_with_mcps(0);
    let area = SettingsPopup::popup_area(Rect::new(0, 0, 100, 30));

    // When
    let hit_areas = popup.tab_hit_areas(area);

    // Then
    assert_eq!(hit_areas.len(), 6);
    assert!(hit_areas[3].x < hit_areas[4].x);
    assert!(hit_areas[4].x < hit_areas[5].x);
}

#[test]
fn activating_preset_provider_row_toggles_provider_enabled_state() {
    // Given
    let mut popup = popup_with_mcps(0);
    popup.active_tab = SettingsTab::Providers;
    popup.db_providers = vec![(
        "OpenAI".to_string(),
        "https://api.openai.com/v1".to_string(),
        "OPENAI_API_KEY".to_string(),
        "openai".to_string(),
        "api_key".to_string(),
    )];
    popup.providers_tab_focus = ProvidersTabFocus::PresetProvider(0);

    // When
    let action = popup.activate_focus();

    // Then
    assert!(matches!(action, ProvidersAction::None));
    assert!(popup.disabled_providers.contains("OpenAI"));
    assert!(popup.preset_key_popup.is_none());
}

#[test]
fn keybindings_help_keeps_vault_route_and_omits_local_route() {
    // Given
    let mut popup = popup_with_mcps(0);
    popup.active_tab = SettingsTab::Keybindings;
    let mut terminal = Terminal::new(TestBackend::new(100, 30)).expect("test terminal");

    // When
    terminal
        .draw(|frame| popup.render(frame))
        .expect("render keybindings");
    let rendered: String = terminal
        .backend()
        .buffer()
        .content
        .iter()
        .map(|cell| cell.symbol())
        .collect();

    // Then
    assert!(rendered.contains("/vault <query>"));
    assert!(!rendered.contains("/local"));
}

#[cfg(feature = "memory")]
#[test]
fn keybindings_help_exposes_memory_routes_when_enabled() {
    let mut popup = popup_with_mcps(0);
    popup.active_tab = SettingsTab::Keybindings;
    let mut terminal = Terminal::new(TestBackend::new(100, 40)).expect("test terminal");

    terminal
        .draw(|frame| popup.render(frame))
        .expect("render keybindings");
    let rendered: String = terminal
        .backend()
        .buffer()
        .content
        .iter()
        .map(|cell| cell.symbol())
        .collect();

    assert!(rendered.contains("@remember"));
    assert!(rendered.contains("@memory / @memorize"));
    assert!(rendered.contains("memory-mcp"));
}

#[test]
fn providers_settings_hide_small_model_controls() {
    let mut popup = popup_with_mcps(0);
    popup.active_tab = SettingsTab::Providers;
    let mut terminal = Terminal::new(TestBackend::new(100, 30)).expect("test terminal");

    terminal
        .draw(|frame| popup.render(frame))
        .expect("render providers");
    let rendered: String = terminal
        .backend()
        .buffer()
        .content
        .iter()
        .map(|cell| cell.symbol())
        .collect();

    assert!(!rendered.contains("Small Provider"));
    assert!(!rendered.contains("Small Model"));
}

#[test]
fn models_tab_renders_provider_and_model_rows() {
    let mut popup = popup_with_mcps(0);
    popup.active_tab = SettingsTab::Models;
    popup.models_provider = "Codex".to_string();
    popup.models_available_models = vec![
        ModelInfo {
            id: "gpt-5.5".to_string(),
            input_price: None,
            output_price: None,
            context_window: Some(400_000),
        },
        ModelInfo {
            id: "gpt-5.4".to_string(),
            input_price: None,
            output_price: None,
            context_window: Some(256_000),
        },
    ];
    let mut terminal = Terminal::new(TestBackend::new(100, 30)).expect("test terminal");

    terminal
        .draw(|frame| popup.render(frame))
        .expect("render models tab");
    let rendered: String = terminal
        .backend()
        .buffer()
        .content
        .iter()
        .map(|cell| cell.symbol())
        .collect();

    assert!(rendered.contains("Models"));
    assert!(rendered.contains("Models For Selected Provider"));
    assert!(rendered.contains("gpt-5.5"));
    assert!(rendered.contains("gpt-5.4"));
}
