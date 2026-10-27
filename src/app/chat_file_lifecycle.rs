use std::path::PathBuf;
use std::sync::{Arc, MutexGuard};

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

struct Environment {
    root: PathBuf,
    previous: [(&'static str, Option<std::ffi::OsString>); 4],
    _guard: MutexGuard<'static, ()>,
}

impl Environment {
    fn new(label: &str) -> Self {
        let guard = crate::test_support::env_lock()
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        let root = std::env::temp_dir().join(format!(
            "tcui-chat-file-lifecycle-{label}-{}",
            rand::random::<u64>()
        ));
        for directory in ["home", "data", "config", "cache"] {
            std::fs::create_dir_all(root.join(directory))
                .expect("create isolated environment directory");
        }
        let environment = Self {
            root,
            previous: ["HOME", "XDG_DATA_HOME", "XDG_CONFIG_HOME", "XDG_CACHE_HOME"]
                .map(|name| (name, std::env::var_os(name))),
            _guard: guard,
        };
        for (name, directory) in [
            ("HOME", "home"),
            ("XDG_DATA_HOME", "data"),
            ("XDG_CONFIG_HOME", "config"),
            ("XDG_CACHE_HOME", "cache"),
        ] {
            std::env::set_var(name, environment.root.join(directory));
        }
        environment
    }

    fn saved_dir(&self) -> PathBuf {
        crate::protection::internal_vault_root()
    }
}

impl Drop for Environment {
    fn drop(&mut self) {
        let _ = crate::protection::lock();
        for (name, value) in &self.previous {
            match value {
                Some(value) => std::env::set_var(name, value),
                None => std::env::remove_var(name),
            }
        }
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

fn start_app() -> TuiApp {
    let config = AppConfig::default();
    TuiApp::new(
        Storage::new().expect("create storage"),
        Arc::new(tokio::sync::RwLock::new(config)),
        Arc::new(LlmClient::new()),
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
