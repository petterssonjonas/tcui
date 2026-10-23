use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use super::{Action, GeneratedFile, Message, TuiApp};
use crate::app::action::{AssistantFinalization, GeneratedFileFinalization};
use crate::config::AppConfig;
use crate::llm::LlmClient;
use crate::storage::Storage;
use crate::ui::artifact_sidebar::ArtifactHandle;

#[path = "chat_file_lifecycle/deletion.rs"]
mod deletion;
#[path = "chat_file_lifecycle/generated.rs"]
mod generated;

fn env_lock() -> &'static Mutex<()> {
    crate::test_support::env_lock()
}

fn unique_root(label: &str) -> PathBuf {
    std::env::temp_dir().join(format!(
        "tcui-chat-file-lifecycle-{label}-{}",
        rand::random::<u64>()
    ))
}

fn configure_environment(root: &Path) {
    let home = root.join("home");
    let data_home = root.join("data");
    let config_home = root.join("config");
    let cache_home = root.join("cache");
    for path in [&home, &data_home, &config_home, &cache_home] {
        std::fs::create_dir_all(path).expect("create isolated environment directory");
    }
    std::env::set_var("HOME", home);
    std::env::set_var("XDG_DATA_HOME", data_home);
    std::env::set_var("XDG_CONFIG_HOME", config_home);
    std::env::set_var("XDG_CACHE_HOME", cache_home);
    std::env::set_var("TCUI_TEST_CHAT_SAVED_LEGACY_RESET", "1");
}

fn app_with_saved_dir(saved_dir: &Path) -> TuiApp {
    let config = AppConfig {
        saved_files_dir: Some(saved_dir.display().to_string()),
        artifact_save_dir: None,
        ..AppConfig::default()
    };
    TuiApp::new(
        Storage::new().expect("create storage"),
        Arc::new(tokio::sync::RwLock::new(config)),
        Arc::new(LlmClient::new()),
        None,
    )
}

fn add_assistant_placeholder(
    app: &mut TuiApp,
    conversation_id: i64,
) -> (crate::app::submission::SubmissionId, usize) {
    let submission = crate::app::submission::InputSubmission::new("fixture", Default::default());
    let request_id = crate::app::submission::PreparedUserTurn::prepare(&submission)
        .expect("prepare fixture")
        .submission_id();
    let message_idx = app.ui.tabs[0].messages.len();
    let history = app.ui.tabs[0].messages.clone();
    app.register_request(
        crate::app::requests::RequestRoute {
            request_id,
            origin_conversation_id: conversation_id,
            destination_conversation_id: conversation_id,
        },
        0,
        history,
    )
    .expect("register fixture request");
    (request_id, message_idx)
}

fn save_report(app: &mut TuiApp, body: &str) -> ArtifactHandle {
    let conversation_id = app.ui.tabs[0].active_conversation;
    let (request_id, _) = add_assistant_placeholder(app, conversation_id);
    let generated = GeneratedFile::from_skill_response(7, conversation_id, "@save report.md", body)
        .expect("parse @save response");
    let message = Message::new(conversation_id, "assistant".to_string(), body.to_string());
    tokio::runtime::Runtime::new()
        .expect("runtime")
        .block_on(
            app.dispatch(Action::FinalizeAssistantMessage(AssistantFinalization {
                request_id,
                message,
                generated_file: GeneratedFileFinalization::Ready(generated),
            })),
        )
        .expect("finalize generated file");
    app.finish_request(request_id)
        .expect("finish generated file request");
    app.ui.tabs[0].temporary_artifacts[0].handle.clone()
}

fn reset_environment(root: &Path) {
    for key in [
        "HOME",
        "XDG_DATA_HOME",
        "XDG_CONFIG_HOME",
        "XDG_CACHE_HOME",
        "TCUI_TEST_CHAT_SAVED_LEGACY_RESET",
    ] {
        std::env::remove_var(key);
    }
    std::fs::remove_dir_all(root).expect("cleanup lifecycle root");
}
