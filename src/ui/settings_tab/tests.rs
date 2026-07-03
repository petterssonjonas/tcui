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
        vault_path: String::new(),
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
fn provider_form_state_submits_trimmed_add_provider_action() {
    // Given
    let mut popup = popup_with_mcps(0);
    popup.active_tab = SettingsTab::Providers;
    popup.providers_tab_focus = ProvidersTabFocus::AddProviderButton;
    assert!(matches!(popup.activate_focus(), ProvidersAction::None));
    let form = popup
        .add_provider_popup
        .as_mut()
        .expect("add provider form");
    form.name = "  Custom AI  ".to_string();
    form.endpoint = "  https://custom.invalid/v1  ".to_string();
    form.api_key = "  secret-token  ".to_string();
    form.focus = ProviderFormFocus::SubmitButton;

    // When
    let action = popup.activate_provider_popup();

    // Then
    match action {
        ProvidersAction::SubmitAdd { provider, api_key } => {
            assert_eq!(provider.name, "Custom AI");
            assert_eq!(provider.endpoint, "https://custom.invalid/v1");
            assert_eq!(provider.backend_type, "openai");
            assert_eq!(api_key, "secret-token");
        }
        other => panic!("expected submit add action, got {other:?}"),
    }
    assert!(popup.add_provider_popup.is_none());
}

#[test]
fn provider_form_state_blocks_duplicate_provider_name() {
    // Given
    let mut popup = popup_with_mcps(0);
    popup.providers_tab_list = vec![EditableProvider {
        name: "Custom AI".to_string(),
        endpoint: "https://existing.invalid/v1".to_string(),
        backend_type: "openai".to_string(),
    }];
    popup.add_provider_popup = Some(ProviderFormState {
        name: "Custom AI".to_string(),
        endpoint: "https://custom.invalid/v1".to_string(),
        api_key: "secret-token".to_string(),
        focus: ProviderFormFocus::SubmitButton,
        ..ProviderFormState::new_add()
    });

    // When
    let action = popup.activate_provider_popup();

    // Then
    assert!(matches!(action, ProvidersAction::None));
    assert!(popup.add_provider_popup.is_some());
}

#[test]
fn next_and_prev_tab_cycle_through_all_settings_tabs() {
    // Given
    let mut popup = popup_with_mcps(0);
    popup.local_focus = LocalFocus::ApiTokenEnv;

    // When / Then
    popup.next_tab();
    assert_eq!(popup.active_tab, SettingsTab::Keybindings);
    popup.next_tab();
    assert_eq!(popup.active_tab, SettingsTab::Providers);
    popup.next_tab();
    assert_eq!(popup.active_tab, SettingsTab::Models);
    popup.next_tab();
    assert_eq!(popup.active_tab, SettingsTab::Local);
    assert_eq!(popup.local_focus, LocalFocus::Enabled);
    popup.next_tab();
    assert_eq!(popup.active_tab, SettingsTab::Mcp);
    popup.next_tab();
    assert_eq!(popup.active_tab, SettingsTab::General);
    popup.prev_tab();
    assert_eq!(popup.active_tab, SettingsTab::Mcp);
}

#[test]
fn local_server_type_cycles_through_all_variants() {
    // Given / When / Then
    assert_eq!(
        super::local::next_local_server_type(LocalServerType::Auto),
        LocalServerType::Ollama
    );
    assert_eq!(
        super::local::next_local_server_type(LocalServerType::Ollama),
        LocalServerType::LlamaCpp
    );
    assert_eq!(
        super::local::next_local_server_type(LocalServerType::LlamaCpp),
        LocalServerType::LmStudio
    );
    assert_eq!(
        super::local::next_local_server_type(LocalServerType::LmStudio),
        LocalServerType::OpenAiCompat
    );
    assert_eq!(
        super::local::next_local_server_type(LocalServerType::OpenAiCompat),
        LocalServerType::Auto
    );
}

#[test]
fn local_text_input_filters_numeric_and_token_fields() {
    // Given
    let mut popup = popup_with_mcps(0);
    popup.active_tab = SettingsTab::Local;

    // When
    popup.local_focus = LocalFocus::Port;
    popup.type_char('x');
    popup.type_char('1');
    popup.type_char('2');
    popup.local_focus = LocalFocus::ApiTokenEnv;
    popup.type_char('A');
    popup.type_char('-');
    popup.type_char('_');
    popup.type_char('9');

    // Then
    assert_eq!(popup.local_port, "12");
    assert_eq!(popup.local_api_token_env, "A_9");
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

#[test]
fn general_settings_edits_vault_path() {
    let mut popup = popup_with_mcps(0);
    popup.active_tab = SettingsTab::General;
    popup.general_focus = GeneralFocus::VaultPath;

    for c in "/tmp/obsidian".chars() {
        popup.type_char(c);
    }
    popup.backspace();

    assert_eq!(popup.vault_path, "/tmp/obsidia");
}

#[test]
fn general_settings_render_exposes_obsidian_vault_path() {
    let mut popup = popup_with_mcps(0);
    popup.active_tab = SettingsTab::General;
    popup.vault_path = "/tmp/obsidian".to_string();
    let mut terminal = Terminal::new(TestBackend::new(100, 40)).expect("test terminal");

    terminal
        .draw(|frame| popup.render(frame))
        .expect("render general settings");
    let rendered: String = terminal
        .backend()
        .buffer()
        .content
        .iter()
        .map(|cell| cell.symbol())
        .collect();

    assert!(rendered.contains("Obsidian vault path"));
    assert!(popup.general_hit_areas.vault_path.is_some());
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
