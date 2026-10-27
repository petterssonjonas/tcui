use super::*;
use crate::llm::chat::ChatRequest;

fn codex_config() -> AppConfig {
    AppConfig {
        providers: crate::config::app_config::provider_presets(),
        default_chat: crate::config::ChatSelection {
            provider: "Codex".to_string(),
            model: "retired-default".to_string(),
            effort: None,
        },
        ..AppConfig::default()
    }
}

fn catalog() -> Vec<crate::ui::ModelInfo> {
    ["z-preferred-current", "a-secondary-current"]
        .into_iter()
        .map(|id| crate::ui::ModelInfo {
            id: id.to_string(),
            input_price: None,
            output_price: None,
            context_window: Some(272_000),
            default_reasoning_effort: Some("high".to_string()),
            supported_reasoning_efforts: vec!["medium".to_string(), "high".to_string()],
        })
        .collect()
}

fn store_catalog(app: &TuiApp, models: &[crate::ui::ModelInfo]) {
    let rows: Vec<crate::storage::db::ModelRow> = models
        .iter()
        .map(|model| {
            (
                model.id.clone(),
                model.input_price,
                model.output_price,
                model.context_window,
                model.default_reasoning_effort.clone(),
                model.supported_reasoning_efforts.clone(),
            )
        })
        .collect();
    app.storage
        .save_models("Codex", &rows)
        .expect("save catalog");
}

fn choose_reasoning_effort(app: &mut TuiApp, effort: &str) {
    app.ui.show_selector = true;
    app.ui.tabs[app.ui.active_tab].reasoning_dropdown_open = true;
    let mut terminal =
        ratatui::Terminal::new(ratatui::backend::TestBackend::new(120, 40)).expect("terminal");
    terminal
        .draw(|frame| app.ui.render(frame))
        .expect("render dropdown");
    let index = app
        .ui
        .current_reasoning_options
        .iter()
        .position(|option| option == effort)
        .expect("supported effort");
    let tab = &app.ui.tabs[app.ui.active_tab];
    let area = tab.dropdown_item_areas[index - tab.dropdown_scroll_offset];
    app.handle_mouse_click(area.x, area.y);
}

#[test]
fn explicit_saved_model_survives_a_missing_catalog_without_advertising_it() {
    with_test_app_config("codex-empty-catalog", codex_config(), |app| {
        assert_eq!(app.ui.tabs[0].tab.provider, "Codex");
        assert_eq!(app.ui.tabs[0].tab.model, "retired-default");
        assert!(app.ui.current_models.is_empty());
    });
}

#[test]
fn refreshed_catalog_replaces_stale_selections_and_reaches_the_responses_payload() {
    with_test_app_config("codex-refresh-selection", codex_config(), |app| {
        app.ui.tabs[0].tab.model = "retired-default".to_string();
        app.ui.tabs[0].tab.reasoning_effort = Some("unsupported-old-effort".to_string());
        app.ui.tabs[0].manual_model_override = false;
        app.ui.add_tab(Tab::new(
            "Second chat".to_string(),
            "Codex".to_string(),
            "another-retired-model".to_string(),
        ));
        app.ui.active_tab = 0;
        let models = catalog();
        store_catalog(app, &models);
        let runtime = tokio::runtime::Runtime::new().expect("create runtime");
        runtime
            .block_on(app.dispatch(Action::SetProviderModels("Codex".to_string(), models)))
            .expect("apply refreshed catalog");

        assert!(app
            .ui
            .tabs
            .iter()
            .all(|tab| tab.tab.model == "z-preferred-current"));
        assert_eq!(
            app.ui.tabs[0].tab.reasoning_effort.as_deref(),
            Some("medium")
        );
        assert_eq!(
            app.ui
                .current_models
                .iter()
                .map(|model| model.id.as_str())
                .collect::<Vec<_>>(),
            ["z-preferred-current", "a-secondary-current"]
        );
        let request = ChatRequest {
            provider: app.ui.tabs[0].tab.provider.clone(),
            endpoint: "https://chatgpt.com/backend-api/codex".to_string(),
            model: app.ui.tabs[0].tab.model.clone(),
            reasoning_effort: app.ui.tabs[0].tab.reasoning_effort.clone(),
            supported_reasoning_efforts: app.ui.current_models[0]
                .supported_reasoning_efforts
                .clone(),
            backend_type: "codex".to_string(),
            api_key: None,
            system_prompt: "test policy".to_string(),
            messages: vec![Message::new(1, "user".to_string(), "hello".to_string())],
        };
        let body: serde_json::Value = serde_json::from_slice(
            &crate::llm::codex_responses::serialize_request_body(&request)
                .expect("serialize request"),
        )
        .expect("decode request");
        assert_eq!(body["model"], "z-preferred-current");
        assert_eq!(body["reasoning"]["effort"], "medium");
        assert_eq!(body["input"][0]["role"], "developer");
        assert!(body.get("instructions").is_none());
    });
}

#[test]
fn explicit_manual_model_survives_catalog_refresh_without_becoming_an_advertised_model() {
    with_test_app_config("codex-manual-refresh", codex_config(), |app| {
        let models = catalog();
        store_catalog(app, &models);
        app.apply_refreshed_provider_models("Codex", &models);
        app.open_manual_model("private-preview-model");
        assert!(app.ui.tabs[0].manual_model_override);

        app.apply_refreshed_provider_models("Codex", &models);
        app.refresh_visible_selectors();

        assert_eq!(app.ui.tabs[0].tab.model, "private-preview-model");
        assert!(!app
            .ui
            .current_models
            .iter()
            .any(|model| model.id == "private-preview-model"));
        assert!(app.ui.tabs[0].tab.reasoning_effort.is_none());
    });
}

#[test]
fn refreshed_catalog_does_not_select_a_disabled_model() {
    with_test_app_config("codex-disabled-default", codex_config(), |app| {
        let models = catalog();
        store_catalog(app, &models);
        app.ui.tabs[0].manual_model_override = false;
        app.ui
            .disabled_models
            .insert(TuiApp::model_disable_key("Codex", "z-preferred-current"));
        app.apply_refreshed_provider_models("Codex", &models);

        assert_eq!(app.ui.tabs[0].tab.model, "a-secondary-current");
        assert_eq!(app.ui.current_models.len(), 1);
    });
}

#[test]
fn refreshing_another_provider_does_not_change_the_current_model() {
    with_test_app_config("codex-unrelated-refresh", codex_config(), |app| {
        let models = catalog();
        store_catalog(app, &models);
        app.ui.tabs[0].manual_model_override = false;
        app.apply_refreshed_provider_models("Codex", &models);
        app.apply_refreshed_provider_models("OpenAI", &[]);

        assert_eq!(app.ui.tabs[0].tab.model, "z-preferred-current");
    });
}

#[test]
fn explicit_selection_survives_metadata_refresh_reopen_and_export_and_rejects_unsupported_effort() {
    with_test_app_config("codex-explicit-refresh", codex_config(), |app| {
        let runtime = tokio::runtime::Runtime::new().expect("create runtime");
        let models = catalog();
        store_catalog(app, &models);
        runtime
            .block_on(app.dispatch(Action::UpdateTabModel(0, "a-secondary-current".to_string())))
            .expect("select model");
        let missing = vec![models[0].clone()];
        store_catalog(app, &missing);
        runtime
            .block_on(app.dispatch(Action::SetProviderModels("Codex".into(), missing)))
            .expect("refresh immediately after model selection");
        assert_eq!(app.ui.tabs[0].tab.model, "a-secondary-current");
        store_catalog(app, &models);
        app.apply_refreshed_provider_models("Codex", &models);
        choose_reasoning_effort(app, "high");
        let expected = crate::config::ChatSelection {
            provider: "Codex".into(),
            model: "a-secondary-current".into(),
            effort: Some("high".into()),
        };
        let conversation_id = app.ui.tabs[0].active_conversation;
        for refreshed in [vec![models[0].clone()], vec![], {
            let mut models = models.clone();
            models[1].supported_reasoning_efforts = vec!["medium".into()];
            models
        }] {
            store_catalog(app, &refreshed);
            runtime
                .block_on(app.dispatch(Action::SetProviderModels("Codex".into(), refreshed)))
                .expect("refresh metadata");
            assert_eq!(app.ui.tabs[0].tab.model, expected.model);
            assert_eq!(app.ui.tabs[0].tab.reasoning_effort, expected.effort);
            let document = app
                .storage
                .read_chat_document(conversation_id)
                .expect("read persisted selection");
            assert_eq!(document.selection.as_ref(), Some(&expected));
            app.load_conversation_into_tab(0, conversation_id)
                .expect("reopen conversation");
            assert_eq!(app.ui.tabs[0].tab.model, expected.model);
            assert_eq!(app.ui.tabs[0].tab.reasoning_effort, expected.effort);
            let exported =
                crate::export::render_chat_document(&document, crate::export::OutputFormat::Json)
                    .expect("export selection");
            let exported: serde_json::Value =
                serde_json::from_str(&exported).expect("decode exported selection");
            assert_eq!(
                exported["selection"],
                serde_json::to_value(&expected).unwrap()
            );
        }
        let messages_before = app.ui.tabs[0].messages.len();
        assert!(runtime
            .block_on(app.dispatch(Action::SendMessage("do not send".into())))
            .is_err());
        assert_eq!(app.ui.tabs[0].messages.len(), messages_before);
        assert!(app.active_request_id().is_none());
    });
}

#[test]
fn an_empty_override_still_initializes_the_provider_model_and_effort() {
    with_test_app_config("codex-empty-override", codex_config(), |app| {
        let models = catalog();
        store_catalog(app, &models);
        app.ui.tabs[0].tab.model.clear();
        app.ui.tabs[0].tab.reasoning_effort = Some("unsupported".into());
        app.ui.tabs[0].manual_model_override = true;
        app.apply_refreshed_provider_models("Codex", &models);
        assert_eq!(app.ui.tabs[0].tab.model, "z-preferred-current");
        assert_eq!(
            app.ui.tabs[0].tab.reasoning_effort.as_deref(),
            Some("medium")
        );
    });
}

#[test]
fn choosing_effort_protects_an_automatically_initialized_model() {
    with_test_app_config("codex-explicit-effort-refresh", codex_config(), |app| {
        let mut models = catalog();
        store_catalog(app, &models);
        app.ui.tabs[0].tab.model.clear();
        app.ui.tabs[0].manual_model_override = false;
        app.refresh_visible_selectors();
        choose_reasoning_effort(app, "high");
        models[0].supported_reasoning_efforts = vec!["medium".into()];
        store_catalog(app, &models);
        app.apply_refreshed_provider_models("Codex", &models);
        assert_eq!(app.ui.tabs[0].tab.reasoning_effort.as_deref(), Some("high"));
        store_catalog(app, &[]);
        app.apply_refreshed_provider_models("Codex", &[]);
        assert_eq!(app.ui.tabs[0].tab.model, "z-preferred-current");
        assert_eq!(app.ui.tabs[0].tab.reasoning_effort.as_deref(), Some("high"));
    });
}
