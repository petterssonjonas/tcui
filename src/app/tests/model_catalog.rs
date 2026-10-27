use super::*;
use crate::llm::chat::ChatRequest;

fn codex_config() -> AppConfig {
    AppConfig {
        providers: crate::config::app_config::provider_presets(),
        default_provider: "Codex".to_string(),
        default_model: "retired-default".to_string(),
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

#[test]
fn codex_without_a_catalog_does_not_advertise_or_send_a_retired_default() {
    with_test_app_config("codex-empty-catalog", codex_config(), |app| {
        assert_eq!(app.ui.tabs[0].tab.provider, "Codex");
        assert!(app.ui.tabs[0].tab.model.is_empty());
        assert!(app.ui.current_models.is_empty());
    });
}

#[test]
fn refreshed_catalog_replaces_stale_selections_and_reaches_the_responses_payload() {
    with_test_app_config("codex-refresh-selection", codex_config(), |app| {
        app.ui.tabs[0].tab.model = "retired-default".to_string();
        app.ui.tabs[0].tab.reasoning_effort = Some("unsupported-old-effort".to_string());
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
        assert_eq!(app.ui.tabs[0].tab.reasoning_effort.as_deref(), Some("high"));
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
        assert_eq!(body["reasoning"]["effort"], "high");
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
fn selecting_a_catalog_model_ends_the_manual_override() {
    with_test_app_config("codex-end-manual", codex_config(), |app| {
        let models = catalog();
        store_catalog(app, &models);
        app.open_manual_model("private-preview-model");
        let runtime = tokio::runtime::Runtime::new().expect("create runtime");
        runtime
            .block_on(app.dispatch(Action::UpdateTabModel(0, "a-secondary-current".to_string())))
            .expect("select advertised model");

        assert!(!app.ui.tabs[0].manual_model_override);
        assert_eq!(app.ui.tabs[0].tab.model, "a-secondary-current");
    });
}

#[test]
fn refreshed_catalog_does_not_select_a_disabled_model() {
    with_test_app_config("codex-disabled-default", codex_config(), |app| {
        let models = catalog();
        store_catalog(app, &models);
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
        app.apply_refreshed_provider_models("Codex", &models);
        app.apply_refreshed_provider_models("OpenAI", &[]);

        assert_eq!(app.ui.tabs[0].tab.model, "z-preferred-current");
    });
}
