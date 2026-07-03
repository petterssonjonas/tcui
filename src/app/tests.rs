use super::*;
use ratatui::layout::Rect;
use std::sync::Mutex;

fn env_lock() -> &'static Mutex<()> {
    crate::test_support::env_lock()
}

fn unique_temp_dir(label: &str) -> std::path::PathBuf {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("system clock before epoch")
        .as_nanos();
    std::env::temp_dir().join(format!("tcui-{label}-{}-{nanos}", std::process::id()))
}

fn with_test_app(label: &str, test: impl FnOnce(&mut TuiApp)) {
    let _guard = env_lock().lock().expect("env lock poisoned");
    let root = unique_temp_dir(label);
    let data_home = root.join("data-home");
    let config_home = root.join("config-home");
    std::fs::create_dir_all(&data_home).expect("create data dir");
    std::fs::create_dir_all(&config_home).expect("create config dir");
    std::env::set_var("XDG_DATA_HOME", &data_home);
    std::env::set_var("XDG_CONFIG_HOME", &config_home);

    let storage = Storage::new().expect("create storage");
    let mut app = TuiApp::new(
        storage,
        Arc::new(tokio::sync::RwLock::new(AppConfig::default())),
        Arc::new(LlmClient::new()),
        None,
    );
    test(&mut app);

    std::fs::remove_dir_all(&root).expect("cleanup temp dir");
    std::env::remove_var("XDG_DATA_HOME");
    std::env::remove_var("XDG_CONFIG_HOME");
}

#[test]
fn mouse_wheel_moves_open_list_popup_before_settings() {
    with_test_app("popup-wheel", |app| {
        // Given
        app.ui.last_area = Some(Rect::new(0, 0, 80, 24));
        app.ui.show_settings = true;
        app.ui.list_popup = Some(crate::ui::modals::list_popup::ListPopup::selectable(
            "Skills",
            "Empty",
            (0..20)
                .map(|idx| {
                    crate::ui::modals::list_popup::ListPopupItem::insert(format!("@skill-{idx} "))
                })
                .collect(),
        ));

        // When
        for _ in 0..15 {
            app.handle_mouse(crossterm::event::MouseEvent {
                kind: crossterm::event::MouseEventKind::ScrollDown,
                column: 0,
                row: 0,
                modifiers: crossterm::event::KeyModifiers::NONE,
            });
        }

        // Then
        let popup = app.ui.list_popup.as_ref().expect("list popup remains open");
        assert_eq!(popup.selected, Some(15));
        assert_eq!(popup.scroll, 2);
        assert_eq!(app.ui.tabs[0].scroll_offset, 0);
    });
}

#[test]
fn slash_routes_keep_vault_and_remove_local() {
    with_test_app("slash-routes", |app| {
        // Given
        app.ui.tabs[0].input_content = "/vault release notes".to_string();

        // When
        let vault_action = app.handle_key(crossterm::event::KeyEvent::new(
            crossterm::event::KeyCode::Enter,
            crossterm::event::KeyModifiers::NONE,
        ));

        // Then
        assert!(matches!(
            vault_action,
            Some(Action::ShowLocalSearch(query)) if query == "release notes"
        ));

        app.ui.tabs[0].input_content = "/local".to_string();
        assert!(matches!(
            app.handle_key(crossterm::event::KeyEvent::new(
                crossterm::event::KeyCode::Enter,
                crossterm::event::KeyModifiers::NONE,
            )),
            Some(Action::SendMessage(message)) if message == "/local"
        ));
    });
}

#[test]
fn skills_popup_labels_description_and_origin_but_inserts_only_name() {
    with_test_app("skills-popup", |app| {
        // Given / When
        app.show_skills_popup();

        // Then
        let popup = app.ui.list_popup.as_ref().expect("skills popup");
        let save = popup
            .items
            .iter()
            .find(|item| item.label.starts_with("@save "))
            .expect("save skill");
        assert!(save.label.contains("Markdown artifact"));
        assert!(save.label.contains("[built-in]"));
        assert_eq!(
            save.action,
            Some(crate::ui::modals::list_popup::ListPopupAction::InsertText(
                "@save ".to_string()
            ))
        );
        assert!(popup
            .items
            .iter()
            .any(|item| item.label.starts_with("@research ")));
    });
}

#[test]
fn save_dialog_mouse_action_writes_the_artifact() {
    with_test_app("artifact-save-click", |app| {
        // Given
        let output = unique_temp_dir("artifact-output").join("demo.md");
        let artifact = crate::ui::artifact_sidebar::ArtifactEntry::temp_markdown(
            1,
            "demo.md".to_string(),
            "# Demo\n\nComplete.".to_string(),
        );
        let mut dialog = crate::ui::modals::save_file::SaveFileDialog::new(
            &artifact,
            output.parent().expect("output parent").to_path_buf(),
            "Save",
        );
        dialog.path_input = output.display().to_string();
        let area = Rect::new(0, 0, 100, 30);
        let mut terminal = ratatui::Terminal::new(ratatui::backend::TestBackend::new(100, 30))
            .expect("test terminal");
        terminal
            .draw(|frame| dialog.render(frame, area))
            .expect("render save dialog");
        let save = dialog.hit_areas.save.expect("save hit area");
        app.ui.last_area = Some(area);
        app.ui.save_file_dialog = Some(dialog);

        // When
        let action = app.handle_mouse_click(save.x, save.y);
        assert!(matches!(action, Some(Action::SaveGeneratedFile)));
        app.save_generated_file().expect("save generated file");

        // Then
        assert_eq!(
            std::fs::read_to_string(&output).expect("read saved artifact"),
            "# Demo\n\nComplete."
        );
        std::fs::remove_dir_all(output.parent().expect("output parent"))
            .expect("cleanup artifact output");
    });
}

#[test]
fn inline_at_completion_discovers_research_skill() {
    with_test_app("research-skill-popup", |app| {
        // Given
        app.ui.tabs[0].input_content = "Use @res".to_string();
        app.ui.tabs[0].input_cursor = app.ui.tabs[0].input_content.chars().count();

        // When
        app.refresh_input_popup();

        // Then
        let popup = app.ui.list_popup.as_ref().expect("skill completion popup");
        assert!(popup
            .items
            .iter()
            .any(|item| item.label.contains("@research")));
    });
}

#[test]
fn media_capture_ignores_existing_non_image_paths() {
    // Given
    let root = std::env::temp_dir().join(format!("tcui-media-capture-{}", rand::random::<u64>()));
    std::fs::create_dir_all(&root).expect("create media fixture");
    let binary = root.join("save");
    let image = root.join("preview.png");
    std::fs::write(&binary, []).expect("write binary fixture");
    std::fs::write(&image, []).expect("write image fixture");
    let content = format!("{}\n![preview]({})", binary.display(), image.display());

    // When
    let sources = local_media_sources(&content);

    // Then
    std::fs::remove_dir_all(root).expect("cleanup media fixture");
    assert_eq!(sources, [image.display().to_string()]);
}

#[test]
fn local_media_sources_finds_file_links_and_skips_duplicates() {
    // Given
    let content = concat!(
        "![preview](file:///tmp/tcui-preview.png)\n",
        "file:///tmp/tcui-preview.png\n",
        "file:///tmp/tcui-second.JPG\n",
        "https://example.com/remote.png\n",
        "file:///tmp/not-media.txt\n",
    );

    // When
    let sources = local_media_sources(content);

    // Then
    assert_eq!(
        sources,
        [
            "file:///tmp/tcui-preview.png".to_string(),
            "file:///tmp/tcui-second.JPG".to_string(),
        ]
    );
}

#[test]
fn find_artifact_searches_all_catalogs() {
    with_test_app("find-artifact", |app| {
        // Given
        let root = unique_temp_dir("artifact-catalog");
        let saved = root.join("saved.md");
        let memory = root.join("memory.md");
        let vault_root = root.join("vault");
        let vault_file = vault_root.join("notes/vault.md");
        std::fs::create_dir_all(vault_file.parent().expect("vault parent"))
            .expect("create vault dirs");
        std::fs::write(&saved, "# Saved").expect("write saved artifact");
        std::fs::write(&memory, "# Memory").expect("write memory artifact");
        std::fs::write(&vault_file, "# Vault").expect("write vault artifact");

        let temporary = crate::ui::artifact_sidebar::ArtifactEntry::temp_markdown(
            42,
            "temp.md".to_string(),
            "# Temp".to_string(),
        );
        let saved_entry = crate::ui::artifact_sidebar::ArtifactEntry::saved_file(saved.clone());
        let memory_entry = crate::ui::artifact_sidebar::ArtifactEntry::memory_file(
            std::path::PathBuf::from("memory.md"),
            "Memory note".to_string(),
            "# Memory".to_string(),
            memory,
        );
        let vault_entry =
            crate::ui::artifact_sidebar::ArtifactEntry::vault_file(&vault_root, &vault_file);

        app.ui.tabs[app.ui.active_tab]
            .temporary_artifacts
            .push(temporary.clone());
        app.ui.saved_artifacts.push(saved_entry.clone());
        app.ui.memory_artifacts.push(memory_entry.clone());
        app.ui.vault_artifacts.push(vault_entry.clone());

        // When / Then
        assert_eq!(
            app.find_artifact(&temporary.handle)
                .map(|artifact| artifact.name),
            Some("temp.md".to_string())
        );
        assert_eq!(
            app.find_artifact(&saved_entry.handle)
                .map(|artifact| artifact.name),
            Some("saved.md".to_string())
        );
        assert_eq!(
            app.find_artifact(&memory_entry.handle)
                .map(|artifact| artifact.name),
            Some("Memory note".to_string())
        );
        assert_eq!(
            app.find_artifact(&vault_entry.handle)
                .map(|artifact| artifact.name),
            Some("notes/vault.md".to_string())
        );

        std::fs::remove_dir_all(root).expect("cleanup artifact catalog");
    });
}

#[test]
fn startup_uses_saved_default_provider_and_model() {
    let _guard = env_lock().lock().expect("env lock poisoned");
    let root = unique_temp_dir("startup-defaults");
    let data_home = root.join("data-home");
    let config_home = root.join("config-home");
    std::fs::create_dir_all(&root).expect("create root dir");
    std::fs::create_dir_all(&data_home).expect("create data dir");
    std::fs::create_dir_all(&config_home).expect("create config dir");
    std::fs::create_dir_all(config_home.join("tcui")).expect("create tcui config dir");
    std::env::set_var("XDG_DATA_HOME", &data_home);
    std::env::set_var("XDG_CONFIG_HOME", &config_home);

    std::fs::write(
        config_home.join("tcui").join("config.toml"),
        r#"
default_provider = "OpenCode Go"
default_model = "deepseek-v4-flash"
"#,
    )
    .expect("write config");

    let storage = Storage::new().expect("create storage");

    let mut app = TuiApp::new(
        storage,
        Arc::new(tokio::sync::RwLock::new(
            AppConfig::load().expect("load config"),
        )),
        Arc::new(LlmClient::new()),
        None,
    );

    assert_eq!(app.ui.tabs[0].tab.provider, "OpenCode Go");
    assert_eq!(app.ui.tabs[0].tab.model, "deepseek-v4-flash");
    app.ui.show_settings = true;
    assert!(matches!(
        app.handle_key(crossterm::event::KeyEvent::new(
            crossterm::event::KeyCode::Esc,
            crossterm::event::KeyModifiers::NONE,
        )),
        Some(Action::CloseSettings)
    ));

    std::fs::remove_dir_all(&root).expect("cleanup temp dir");
    std::env::remove_var("XDG_DATA_HOME");
    std::env::remove_var("XDG_CONFIG_HOME");
}

#[test]
fn startup_adds_local_inference_provider_when_enabled() {
    let _guard = env_lock().lock().expect("env lock poisoned");
    let root = unique_temp_dir("startup-local");
    let data_home = root.join("data-home");
    let config_home = root.join("config-home");
    std::fs::create_dir_all(&root).expect("create root dir");
    std::fs::create_dir_all(&data_home).expect("create data dir");
    std::fs::create_dir_all(&config_home).expect("create config dir");
    std::fs::create_dir_all(config_home.join("tcui")).expect("create tcui config dir");
    std::env::set_var("XDG_DATA_HOME", &data_home);
    std::env::set_var("XDG_CONFIG_HOME", &config_home);

    std::fs::write(
        config_home.join("tcui").join("config.toml"),
        r#"
default_provider = "Local Inference"
default_model = ""

[local_inference]
enabled = true
host = "127.0.0.1"
port = 11434
server_type = "auto"
selected_model = "llama3.1"
"#,
    )
    .expect("write config");

    let storage = Storage::new().expect("create storage");
    let app = TuiApp::new(
        storage,
        Arc::new(tokio::sync::RwLock::new(
            AppConfig::load().expect("load config"),
        )),
        Arc::new(LlmClient::new()),
        None,
    );

    assert!(app
        .ui
        .db_providers
        .iter()
        .any(|(name, _, _, _, _)| name == crate::config::LOCAL_PROVIDER_NAME));
    assert_eq!(
        app.ui.tabs[0].tab.provider,
        crate::config::LOCAL_PROVIDER_NAME
    );
    assert_eq!(app.ui.tabs[0].tab.model, "llama3.1");

    std::fs::remove_dir_all(&root).expect("cleanup temp dir");
    std::env::remove_var("XDG_DATA_HOME");
    std::env::remove_var("XDG_CONFIG_HOME");
}

#[test]
fn typing_slash_opens_command_popup() {
    with_test_app("slash-popup", |app| {
        app.handle_key(crossterm::event::KeyEvent::new(
            crossterm::event::KeyCode::Char('/'),
            crossterm::event::KeyModifiers::NONE,
        ));

        let popup = app.ui.list_popup.as_ref().expect("command popup");
        assert_eq!(popup.title, "Commands");
        assert!(popup.items.iter().any(|item| item.label.contains("/theme")));
        assert!(popup
            .items
            .iter()
            .any(|item| item.label.contains("/remindme")));
    });
}

#[test]
fn slash_popup_allows_continued_typing() {
    with_test_app("slash-popup-typing", |app| {
        app.handle_key(crossterm::event::KeyEvent::new(
            crossterm::event::KeyCode::Char('/'),
            crossterm::event::KeyModifiers::NONE,
        ));
        app.handle_key(crossterm::event::KeyEvent::new(
            crossterm::event::KeyCode::Char('t'),
            crossterm::event::KeyModifiers::NONE,
        ));

        let input = &app.ui.tabs[0].input_content;
        let popup = app.ui.list_popup.as_ref().expect("command popup");
        assert_eq!(input, "/t");
        assert_eq!(popup.title, "Commands");
    });
}

#[test]
fn arrow_keys_move_cursor_and_insert_in_place() {
    with_test_app("input-cursor", |app| {
        app.handle_key(crossterm::event::KeyEvent::new(
            crossterm::event::KeyCode::Char('a'),
            crossterm::event::KeyModifiers::NONE,
        ));
        app.handle_key(crossterm::event::KeyEvent::new(
            crossterm::event::KeyCode::Char('c'),
            crossterm::event::KeyModifiers::NONE,
        ));
        app.handle_key(crossterm::event::KeyEvent::new(
            crossterm::event::KeyCode::Left,
            crossterm::event::KeyModifiers::NONE,
        ));
        app.handle_key(crossterm::event::KeyEvent::new(
            crossterm::event::KeyCode::Char('b'),
            crossterm::event::KeyModifiers::NONE,
        ));

        let tab = &app.ui.tabs[0];
        assert_eq!(tab.input_content, "abc");
        assert_eq!(tab.input_cursor, 2);
    });
}

#[test]
fn alt_scrolls_lines_and_shift_jumps_answers() {
    with_test_app("answer-jump", |app| {
        let tab = &mut app.ui.tabs[0];
        tab.scroll_offset = 4;
        tab.total_rendered_lines = 60;
        tab.message_viewport_height = 10;
        tab.messages = vec![
            crate::app::message::Message::new(1, "user".to_string(), "Q1".to_string()),
            crate::app::message::Message::new(1, "assistant".to_string(), "A1".to_string()),
            crate::app::message::Message::new(1, "user".to_string(), "Q2".to_string()),
            crate::app::message::Message::new(1, "assistant".to_string(), "A2".to_string()),
        ];
        tab.answer_anchor_lines = vec![(1, 3), (3, 20)];

        app.handle_key(crossterm::event::KeyEvent::new(
            crossterm::event::KeyCode::Down,
            crossterm::event::KeyModifiers::ALT,
        ));
        assert_eq!(app.ui.tabs[0].scroll_offset, 7);

        app.handle_key(crossterm::event::KeyEvent::new(
            crossterm::event::KeyCode::Down,
            crossterm::event::KeyModifiers::SHIFT,
        ));
        assert_eq!(app.ui.tabs[0].scroll_to_message, Some(3));
    });
}

#[cfg(feature = "memory")]
#[test]
fn inline_at_completion_discovers_memory_skills() {
    with_test_app("memory-skill-popup", |app| {
        // Given
        app.ui.tabs[0].input_content = "Use @rem".to_string();
        app.ui.tabs[0].input_cursor = app.ui.tabs[0].input_content.chars().count();

        // When
        app.refresh_input_popup();

        // Then
        let popup = app.ui.list_popup.as_ref().expect("skill completion popup");
        assert_eq!(popup.title, "Skills");
        assert!(popup
            .items
            .iter()
            .any(|item| item.label.contains("@remember")));
    });
}

#[test]
fn theme_command_persists_selection() {
    with_test_app("theme-command", |app| {
        if let Some(tab) = app.ui.tabs.get_mut(0) {
            tab.input_content = "/theme nord".to_string();
        }

        let action = app.handle_key(crossterm::event::KeyEvent::new(
            crossterm::event::KeyCode::Enter,
            crossterm::event::KeyModifiers::NONE,
        ));

        assert!(action.is_none());
        let live_theme = app
            .config
            .try_read()
            .map(|config| config.theme.clone())
            .unwrap_or_default();
        assert_eq!(live_theme, "nord");
    });
}

#[test]
fn send_message_creates_conversation_and_saves_user_message() {
    with_test_app("send-message", |app| {
        // Given
        let initial_conversation = app.ui.tabs[0].active_conversation;

        // When
        app.send_message("Hello from test".to_string())
            .expect("send message");

        // Then
        let conversation_id = app.ui.tabs[0].active_conversation;
        assert!(conversation_id > 0);
        if initial_conversation > 0 {
            assert_eq!(conversation_id, initial_conversation);
        }
        let stored = app
            .storage
            .get_messages(conversation_id)
            .expect("stored messages");
        assert_eq!(stored.len(), 1);
        assert_eq!(stored[0].role, "user");
        assert_eq!(stored[0].content, "Hello from test");
        assert_eq!(
            app.ui.tabs[0].generated_title.as_deref(),
            Some("Hello from test")
        );
    });
}

#[test]
fn slash_remindme_is_handled_without_model_streaming() {
    with_test_app("slash-remindme", |app| {
        std::env::set_var("TCUI_REMINDER_SYSTEMD_RUN", "true");

        app.send_message("/remindme in 10m | Stretch".to_string())
            .expect("schedule reminder");

        let tab = &app.ui.tabs[0];
        assert_eq!(tab.messages.len(), 2);
        assert_eq!(tab.messages[0].role, "user");
        assert_eq!(tab.messages[1].role, "assistant");
        assert!(tab.messages[1]
            .content
            .contains("Scheduled one-shot reminder"));

        std::env::remove_var("TCUI_REMINDER_SYSTEMD_RUN");
    });
}

#[test]
fn settings_popup_state_loads_disabled_items_and_saves_local_fields() {
    with_test_app("settings-state", |app| {
        // Given
        let mut config = AppConfig {
            default_provider: "Custom".to_string(),
            default_model: "custom-model".to_string(),
            theme: "nord".to_string(),
            providers: vec![crate::config::ProviderConfig {
                name: "Custom".to_string(),
                endpoint: "https://example.invalid/v1".to_string(),
                env_var: "CUSTOM_API_KEY".to_string(),
                backend_type: "openai".to_string(),
                auth_type: "api_key".to_string(),
            }],
            disabled_providers: vec!["Custom".to_string()],
            disabled_models: vec!["custom-model".to_string()],
            ..AppConfig::default()
        };
        config.local_inference.enabled = true;
        config.local_inference.port = 11434;

        // When
        let mut popup = app.load_settings_popup_state(&config);

        // Then
        assert!(popup.disabled_providers.contains("Custom"));
        assert!(popup.disabled_models.contains("custom-model"));
        assert_eq!(popup.local_port, "11434");

        // When
        popup.local_port = "12345".to_string();
        popup.local_health_interval_seconds = "9".to_string();
        popup.local_connect_timeout_ms = "700".to_string();
        popup.local_request_timeout_ms = "1700".to_string();
        popup.local_api_token_env = "LOCAL_TOKEN".to_string();
        popup.theme = "dracula".to_string();
        tokio::runtime::Runtime::new()
            .expect("runtime")
            .block_on(app.save_settings_popup_state(&popup))
            .expect("save settings");

        // Then
        let saved = AppConfig::load().expect("load saved settings");
        assert_eq!(saved.local_inference.port, 12345);
        assert_eq!(saved.local_inference.health_check_interval_seconds, 9);
        assert_eq!(saved.local_inference.connect_timeout_ms, 700);
        assert_eq!(saved.local_inference.request_timeout_ms, 1700);
        assert_eq!(
            saved.local_inference.api_token_env.as_deref(),
            Some("LOCAL_TOKEN")
        );
        assert_eq!(saved.theme, "dracula");
    });
}

#[test]
fn apply_theme_selection_updates_popup_and_saved_config() {
    with_test_app("theme-selection", |app| {
        // Given
        app.ui.settings_popup = Some(app.load_settings_popup_state(&AppConfig::default()));

        // When
        app.apply_theme_selection("nord").expect("apply theme");

        // Then
        let live_theme = app
            .config
            .try_read()
            .map(|config| config.theme.clone())
            .expect("read live config");
        assert_eq!(live_theme, "nord");
        assert_eq!(
            app.ui
                .settings_popup
                .as_ref()
                .map(|popup| popup.theme.as_str()),
            Some("nord")
        );
        assert_eq!(AppConfig::load().expect("load config").theme, "nord");
    });
}

#[tokio::test]
async fn oauth_connection_check_skips_models_probe() {
    let mut config = AppConfig::default();
    config.default_provider = "Codex".to_string();

    let result = TuiApp::check_cloud_connection("Codex", &config, Some("token")).await;

    assert!(result.is_ok());
}

#[test]
fn startup_loads_persisted_conversation_history() {
    let _guard = env_lock().lock().expect("env lock poisoned");
    let root = unique_temp_dir("startup-history");
    let data_home = root.join("data-home");
    let config_home = root.join("config-home");
    std::fs::create_dir_all(&data_home).expect("create data dir");
    std::fs::create_dir_all(&config_home).expect("create config dir");
    std::env::set_var("XDG_DATA_HOME", &data_home);
    std::env::set_var("XDG_CONFIG_HOME", &config_home);

    let storage = Storage::new().expect("create storage");
    let conversation_id = storage.create_conversation(0).expect("create conversation");
    storage
        .update_conversation_title(conversation_id, "Persisted title")
        .expect("update title");
    let message = Message::new(
        conversation_id,
        "user".to_string(),
        "Persisted question".to_string(),
    );
    storage.save_message(&message).expect("save message");

    let app = TuiApp::new(
        storage,
        Arc::new(tokio::sync::RwLock::new(AppConfig::default())),
        Arc::new(LlmClient::new()),
        None,
    );

    assert_eq!(app.ui.tabs[0].active_conversation, conversation_id);
    assert_eq!(app.ui.tabs[0].conversations.len(), 1);
    assert_eq!(app.ui.tabs[0].conversations[0].title, "Persisted title");
    assert_eq!(
        app.ui.tabs[0].generated_title.as_deref(),
        Some("Persisted title")
    );
    assert_eq!(app.ui.tabs[0].messages.len(), 1);
    assert_eq!(app.ui.tabs[0].messages[0].content, "Persisted question");

    std::fs::remove_dir_all(&root).expect("cleanup temp dir");
    std::env::remove_var("XDG_DATA_HOME");
    std::env::remove_var("XDG_CONFIG_HOME");
}

#[test]
fn closing_active_chat_deletes_it_and_loads_the_next_persisted_conversation() {
    let _guard = env_lock().lock().expect("env lock poisoned");
    let root = unique_temp_dir("close-chat");
    let data_home = root.join("data-home");
    let config_home = root.join("config-home");
    std::fs::create_dir_all(&data_home).expect("create data dir");
    std::fs::create_dir_all(&config_home).expect("create config dir");
    std::env::set_var("XDG_DATA_HOME", &data_home);
    std::env::set_var("XDG_CONFIG_HOME", &config_home);

    let storage = Storage::new().expect("create storage");
    let first = storage.create_conversation(0).expect("first conversation");
    storage
        .update_conversation_title(first, "First chat")
        .expect("first title");
    storage
        .save_message(&Message::new(
            first,
            "user".to_string(),
            "First message".to_string(),
        ))
        .expect("save first message");

    let second = storage.create_conversation(0).expect("second conversation");
    storage
        .update_conversation_title(second, "Second chat")
        .expect("second title");
    storage
        .save_message(&Message::new(
            second,
            "user".to_string(),
            "Second message".to_string(),
        ))
        .expect("save second message");

    let mut app = TuiApp::new(
        storage,
        Arc::new(tokio::sync::RwLock::new(AppConfig::default())),
        Arc::new(LlmClient::new()),
        None,
    );
    assert_eq!(app.ui.tabs[0].active_conversation, second);

    tokio::runtime::Runtime::new()
        .expect("runtime")
        .block_on(app.dispatch(Action::CloseChat))
        .expect("close chat");

    assert_eq!(app.ui.tabs[0].active_conversation, first);
    assert_eq!(app.ui.tabs[0].messages.len(), 1);
    assert_eq!(app.ui.tabs[0].messages[0].content, "First message");
    assert_eq!(
        app.storage
            .get_conversations(0)
            .expect("stored conversations")
            .len(),
        1
    );

    std::fs::remove_dir_all(&root).expect("cleanup temp dir");
    std::env::remove_var("XDG_DATA_HOME");
    std::env::remove_var("XDG_CONFIG_HOME");
}

#[test]
fn toggle_conversation_pinned_reorders_sidebar_catalog() {
    with_test_app("pin-conversation", |app| {
        let first = app
            .storage
            .create_conversation(0)
            .expect("first conversation");
        app.storage
            .update_conversation_title(first, "First chat")
            .expect("first title");
        let second = app
            .storage
            .create_conversation(0)
            .expect("second conversation");
        app.storage
            .update_conversation_title(second, "Second chat")
            .expect("second title");

        app.refresh_tab_conversations(0)
            .expect("refresh conversations");
        assert_eq!(app.ui.tabs[0].conversations[0].id, second);

        app.toggle_conversation_pinned(first)
            .expect("toggle pinned");

        assert_eq!(app.ui.tabs[0].conversations[0].id, first);
        assert!(app.ui.tabs[0].conversations[0].pinned);
    });
}

#[test]
fn deleting_inactive_conversation_keeps_active_chat_loaded() {
    with_test_app("delete-conversation", |app| {
        let first = app
            .storage
            .create_conversation(0)
            .expect("first conversation");
        app.storage
            .update_conversation_title(first, "First chat")
            .expect("first title");
        app.storage
            .save_message(&Message::new(
                first,
                "user".to_string(),
                "First message".to_string(),
            ))
            .expect("save first message");

        let second = app
            .storage
            .create_conversation(0)
            .expect("second conversation");
        app.storage
            .update_conversation_title(second, "Second chat")
            .expect("second title");
        app.load_conversation_into_tab(0, second)
            .expect("load second conversation");
        app.refresh_tab_conversations(0)
            .expect("refresh conversations");

        app.delete_conversation_by_id(first)
            .expect("delete first conversation");

        assert_eq!(app.ui.tabs[0].active_conversation, second);
        assert!(app.ui.tabs[0]
            .conversations
            .iter()
            .all(|conversation| conversation.id != first));
        assert!(app.ui.tabs[0]
            .conversations
            .iter()
            .any(|conversation| conversation.id == second));
    });
}

#[test]
fn export_dialog_exports_current_conversation_as_json() {
    with_test_app("export-dialog", |app| {
        let conversation_id = app.storage.create_conversation(0).expect("conversation");
        app.storage
            .update_conversation_title(conversation_id, "Export me")
            .expect("title");
        app.storage
            .save_message(&Message::new(
                conversation_id,
                "user".to_string(),
                "Exported content".to_string(),
            ))
            .expect("message");
        app.load_conversation_into_tab(0, conversation_id)
            .expect("load conversation");

        let output_dir = unique_temp_dir("export-output");
        std::fs::create_dir_all(&output_dir).expect("output dir");
        app.open_conversation_export_dialog();
        let mut dialog = app.ui.export_dialog.clone().expect("export dialog");
        dialog.directory_input = output_dir.display().to_string();

        let area = Rect::new(0, 0, 100, 30);
        let mut terminal = ratatui::Terminal::new(ratatui::backend::TestBackend::new(100, 30))
            .expect("test terminal");
        terminal
            .draw(|frame| dialog.render(frame, area))
            .expect("render export dialog");
        let json = dialog.hit_areas.json.expect("json hit area");
        let export = dialog.hit_areas.export.expect("export hit area");
        app.ui.last_area = Some(area);
        app.ui.export_dialog = Some(dialog);

        assert!(app.handle_mouse_click(json.x, json.y).is_none());
        let action = app.handle_mouse_click(export.x, export.y);
        assert!(matches!(action, Some(Action::SaveExportDialog)));
        app.save_export_dialog().expect("export conversation");

        let exported = std::fs::read_dir(&output_dir)
            .expect("read output dir")
            .map(|entry| entry.expect("dir entry").path())
            .find(|path| path.extension().and_then(|value| value.to_str()) == Some("json"))
            .expect("exported json");
        let content = std::fs::read_to_string(&exported).expect("read exported file");
        assert!(content.contains("\"title\": \"Export me\""));
        assert!(content.contains("Exported content"));
        std::fs::remove_dir_all(&output_dir).expect("cleanup output dir");
    });
}

#[test]
fn export_dialog_can_open_for_non_active_conversation() {
    with_test_app("export-other-conversation", |app| {
        let first = app
            .storage
            .create_conversation(0)
            .expect("first conversation");
        app.storage
            .update_conversation_title(first, "First chat")
            .expect("first title");
        let second = app
            .storage
            .create_conversation(0)
            .expect("second conversation");
        app.storage
            .update_conversation_title(second, "Second chat")
            .expect("second title");
        app.refresh_tab_conversations(0)
            .expect("refresh conversations");

        app.open_conversation_export_dialog_for(first);

        let dialog = app.ui.export_dialog.clone().expect("export dialog");
        assert!(matches!(
            dialog.target,
            crate::ui::modals::export_dialog::ExportTarget::Conversation(id) if id == first
        ));
        assert_eq!(dialog.item_name, "First chat");
    });
}

#[test]
fn arrow_up_and_down_browse_previous_user_prompts() {
    with_test_app("prompt-history", |app| {
        app.ui.tabs[0]
            .messages
            .push(crate::app::message::Message::new(
                1,
                "user".to_string(),
                "first prompt".to_string(),
            ));
        app.ui.tabs[0]
            .messages
            .push(crate::app::message::Message::new(
                1,
                "assistant".to_string(),
                "answer".to_string(),
            ));
        app.ui.tabs[0]
            .messages
            .push(crate::app::message::Message::new(
                1,
                "user".to_string(),
                "second prompt".to_string(),
            ));
        app.ui.tabs[0].input_content = "draft".to_string();
        app.ui.tabs[0].input_cursor = 5;

        app.handle_key(crossterm::event::KeyEvent::new(
            crossterm::event::KeyCode::Up,
            crossterm::event::KeyModifiers::NONE,
        ));
        assert_eq!(app.ui.tabs[0].input_content, "second prompt");

        app.handle_key(crossterm::event::KeyEvent::new(
            crossterm::event::KeyCode::Up,
            crossterm::event::KeyModifiers::NONE,
        ));
        assert_eq!(app.ui.tabs[0].input_content, "first prompt");

        app.handle_key(crossterm::event::KeyEvent::new(
            crossterm::event::KeyCode::Down,
            crossterm::event::KeyModifiers::NONE,
        ));
        assert_eq!(app.ui.tabs[0].input_content, "second prompt");

        app.handle_key(crossterm::event::KeyEvent::new(
            crossterm::event::KeyCode::Down,
            crossterm::event::KeyModifiers::NONE,
        ));
        assert_eq!(app.ui.tabs[0].input_content, "draft");
    });
}

#[test]
fn insert_input_char_adds_character_at_cursor() {
    with_test_app("input-char", |app| {
        app.ui.tabs[0].input_content = "hel".to_string();
        app.ui.tabs[0].input_cursor = 3;

        app.insert_input_char('l');

        assert_eq!(app.ui.tabs[0].input_content, "hell");
        assert_eq!(app.ui.tabs[0].input_cursor, 4);
    });
}

#[test]
fn insert_input_char_at_start_prepends() {
    with_test_app("input-start", |app| {
        app.ui.tabs[0].input_content = "world".to_string();
        app.ui.tabs[0].input_cursor = 0;

        app.insert_input_char('h');

        assert_eq!(app.ui.tabs[0].input_content, "hworld");
        assert_eq!(app.ui.tabs[0].input_cursor, 1);
    });
}

#[test]
fn backspace_input_char_removes_previous_character() {
    with_test_app("input-backspace", |app| {
        app.ui.tabs[0].input_content = "hello".to_string();
        app.ui.tabs[0].input_cursor = 5;

        app.backspace_input_char();

        assert_eq!(app.ui.tabs[0].input_content, "hell");
        assert_eq!(app.ui.tabs[0].input_cursor, 4);
    });
}

#[test]
fn backspace_at_start_does_nothing() {
    with_test_app("input-backspace-start", |app| {
        app.ui.tabs[0].input_content = "hello".to_string();
        app.ui.tabs[0].input_cursor = 0;

        app.backspace_input_char();

        assert_eq!(app.ui.tabs[0].input_content, "hello");
        assert_eq!(app.ui.tabs[0].input_cursor, 0);
    });
}

#[test]
fn delete_input_char_removes_next_character() {
    with_test_app("input-delete", |app| {
        app.ui.tabs[0].input_content = "hello".to_string();
        app.ui.tabs[0].input_cursor = 1;

        app.delete_input_char();

        assert_eq!(app.ui.tabs[0].input_content, "hllo");
        assert_eq!(app.ui.tabs[0].input_cursor, 1);
    });
}

#[test]
fn move_input_cursor_respects_content_bounds() {
    with_test_app("input-cursor", |app| {
        app.ui.tabs[0].input_content = "hi".to_string();
        app.ui.tabs[0].input_cursor = 1;

        app.move_input_cursor_left();
        assert_eq!(app.ui.tabs[0].input_cursor, 0);

        app.move_input_cursor_left();
        assert_eq!(app.ui.tabs[0].input_cursor, 0);

        app.move_input_cursor_right();
        assert_eq!(app.ui.tabs[0].input_cursor, 1);

        app.move_input_cursor_right();
        assert_eq!(app.ui.tabs[0].input_cursor, 2);

        app.move_input_cursor_right();
        assert_eq!(app.ui.tabs[0].input_cursor, 2);
    });
}

#[cfg(feature = "memory")]
#[tokio::test]
async fn refresh_artifact_sidebar_action_populates_memory_artifacts() {
    let _guard = env_lock().lock().expect("env lock poisoned");
    let root = unique_temp_dir("memory-sidebar");
    let data_home = root.join("data-home");
    let config_home = root.join("config-home");
    let vault = root.join("vault");
    std::fs::create_dir_all(&data_home).expect("create data dir");
    std::fs::create_dir_all(&config_home).expect("create config dir");
    std::fs::create_dir_all(&vault).expect("create vault dir");
    std::env::set_var("XDG_DATA_HOME", &data_home);
    std::env::set_var("XDG_CONFIG_HOME", &config_home);

    let mut config = AppConfig::default();
    config.memory.enabled = true;
    config.vault_path = Some(vault.to_string_lossy().to_string());
    config.save().expect("save config");

    let storage = Storage::new().expect("create storage");
    let mut app = TuiApp::new(
        storage,
        Arc::new(tokio::sync::RwLock::new(config)),
        Arc::new(crate::llm::LlmClient::new()),
        Some(Arc::new(crate::obsidian::Vault::new(vault.clone()))),
    );

    let store =
        crate::memory::MemoryStore::open(&vault, &crate::memory::MemoryStore::default_cache_path())
            .expect("open memory store");
    store
        .remember("User prefers Rust for systems work.")
        .expect("save memory");

    assert!(app.ui.memory_artifacts.is_empty());

    app.dispatch(Action::RefreshArtifactSidebar)
        .await
        .expect("dispatch refresh");

    assert_eq!(app.ui.memory_artifacts.len(), 1);
    assert_eq!(
        app.ui.memory_artifacts[0].content.as_deref(),
        Some("User prefers Rust for systems work.")
    );

    std::fs::remove_dir_all(&root).expect("cleanup temp dir");
    std::env::remove_var("XDG_DATA_HOME");
    std::env::remove_var("XDG_CONFIG_HOME");
}
