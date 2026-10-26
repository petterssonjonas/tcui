#![cfg(target_os = "linux")]

use std::ffi::OsString;
use std::future::Future;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::time::Duration;

use serde_json::{json, Value};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};

use super::{run, AgentOutcome};
use crate::app::Message;
use crate::config::key_store::CredentialStorage;
use crate::config::{AppConfig, KeyStore};
use crate::llm::chat::ChatRequest;
use crate::notifications::pushover::{APP_KEY_ID, USER_KEY_ID};

const USER: &str = "uuuuuuuuuuuuuuuuuuuuuuuuuuuuuu";
const APP: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const DEADLINE: Duration = Duration::from_secs(10);

#[test]
fn model_uses_history_and_composed_skills_to_register_once_then_cancel() {
    reminder_scenario(false);
}

#[test]
fn successful_registration_survives_a_later_model_transport_failure() {
    reminder_scenario(true);
}

fn reminder_scenario(fail_after_create: bool) {
    with_fixture("host-agent-reminder", |root, config| {
        save_keys(config, CredentialStorage::Plaintext);
        let curl = helper(root, "curl", "printf immediate > \"$HOME/immediate\"");
        config.commands.whitelist = vec![curl.to_str().unwrap().to_owned()];
        let scheduler = helper(root, "scheduler", "printf 'registered\\n' >> \"$HOME/registrations\"\nprintf '%s\\n' \"$@\" > \"$HOME/scheduler-argv\"");
        let controller = helper(
            root,
            "controller",
            "printf '%s\\n' \"$*\" >> \"$HOME/cancellations\"",
        );
        let _env = Environment::set(&[
            ("TCUI_REMINDER_SYSTEMD_RUN", scheduler.into_os_string()),
            ("TCUI_REMINDER_SYSTEMCTL", controller.into_os_string()),
            (
                "TCUI_REMINDER_SYSTEMD_ANALYZE",
                OsString::from("/usr/bin/false"),
            ),
            ("PATH", root.as_os_str().to_owned()),
        ]);
        block_on(async {
            let raw = "@remindme via @ping-user in twenty minutes to look into that subject";
            let create = json!({"tool":"reminder","arguments":{
                "action":"create","message":"look into subject X","after_seconds":1200,"pushover":true
            }});
            let replies = if fail_after_create {
                vec![
                    Reply::Json(create),
                    Reply::Failure("fixture provider unavailable".into()),
                ]
            } else {
                vec![Reply::Json(create.clone()), Reply::Json(create), finish()]
            };
            let (endpoint, server) = model(replies).await;
            let mut request = request(&endpoint, raw);
            request.messages.insert(
                0,
                Message::new(1, "user".into(), "The subject is subject X.".into()),
            );
            request.messages.insert(
                1,
                Message::new(
                    2,
                    "assistant".into(),
                    "We will investigate subject X.".into(),
                ),
            );
            let prepared = crate::skill_runtime::prepare(config, raw, &request).await;
            assert!(prepared.notices.is_empty());
            request.system_prompt.push_str(&prepared.context);
            let outcome = run(config, raw, &request).await;
            let requests = server.await.unwrap();
            let first = requests[0].to_string();
            assert!(first.contains("The subject is subject X."));
            assert!(first.contains("We will investigate subject X."));
            assert!(first.contains(raw));
            let catalog = crate::skills::SkillCatalog::discover().unwrap();
            for name in ["remindme", "ping-user"] {
                let source = catalog.load(name).unwrap().unwrap().source;
                assert!(
                    requests[0]["messages"]
                        .as_array()
                        .unwrap()
                        .iter()
                        .any(|message| {
                            message["content"]
                                .as_str()
                                .is_some_and(|content| content.contains(source.as_ref()))
                        }),
                    "the actual composed skill body must reach the transport"
                );
            }
            let observation = last_observation(&requests[1]);
            assert_eq!(observation["success"], true);
            assert!(observation["result"]["message"]
                .as_str()
                .unwrap()
                .contains("subject X"));
            if !fail_after_create {
                assert_eq!(
                    last_observation(&requests[2])["result"]["already_executed"],
                    true
                );
                assert!(outcome.notices.is_empty());
            } else {
                assert!(!outcome.notices.is_empty());
            }
            assert!(visible(&outcome).contains("subject X"));
            assert_eq!(
                std::fs::read_to_string(root.join("registrations")).unwrap(),
                "registered\n"
            );
            let scheduled_argv = std::fs::read_to_string(root.join("scheduler-argv")).unwrap();
            assert!(scheduled_argv.contains("--on-active\n1200s\n"));
            assert!(scheduled_argv.contains("\nreminder-ping-dispatch\n"));
            assert!(
                !root.join("immediate").exists(),
                "future requests must not send immediately"
            );
            let records = records();
            assert_eq!(records.len(), 1);
            assert!(records[0].pushover);
            assert!(!records[0].recurring);
            let id = &records[0].id;
            let listed = crate::reminders::agent_tool(config, &json!({"action":"list"})).unwrap();
            assert!(listed.contains(id) && listed.contains("look into subject X"));
            let pending = data_root()
                .join("reminder-notifications")
                .join(format!("{id}.json"));
            let snapshot = std::fs::read_to_string(&pending).unwrap();
            assert!(
                snapshot.contains("look into subject X")
                    && snapshot.contains(USER)
                    && snapshot.contains(APP)
            );

            let (endpoint, server) = model(vec![
                Reply::Json(json!({"tool":"reminder","arguments":{
                    "action":"cancel","id":id
                }})),
                finish(),
            ])
            .await;
            let raw = "@remindme cancel the registered reminder";
            let cancelled = run(config, raw, &request_for_cancel(&endpoint, raw, &outcome)).await;
            let requests = server.await.unwrap();
            assert_eq!(last_observation(&requests[1])["success"], true);
            assert!(cancelled.notices.is_empty());
            assert!(records_empty());
            assert!(!pending.exists());
            assert!(std::fs::read_to_string(root.join("cancellations"))
                .unwrap()
                .contains(&records[0].unit));
        });
    });
}

#[test]
fn command_argv_is_literal_and_policy_denials_have_no_side_effect() {
    with_fixture("host-agent-command", |root, config| {
        let program = helper(
            root,
            "argv",
            "output=$1; shift\nprintf '%s\\n' \"$@\" > \"$output\"",
        );
        let blocked = helper(root, "blocked", "printf forbidden > \"$1\"");
        let unapproved = helper(root, "unapproved", "printf forbidden > \"$1\"");
        config.commands.whitelist =
            vec![program.display().to_string(), blocked.display().to_string()];
        config.commands.blacklist = vec![blocked.display().to_string()];
        block_on(async {
            let output = root.join("literal-argv");
            let forbidden = root.join("forbidden");
            let literal = format!("$(touch {}); * $HOME 'quoted'", forbidden.display());
            let (endpoint, server) = model(vec![
                command(&program, json!([output, literal, "two words", ""])),
                command(&blocked, json!([forbidden])),
                command(&unapproved, json!([forbidden])),
                finish(),
            ])
            .await;
            let raw = "@command record the literal arguments, then try the restricted helpers";
            let outcome = run(config, raw, &request(&endpoint, raw)).await;
            assert!(
                outcome.notices.is_empty(),
                "agent failed before fixture completion: {:?}",
                outcome.notices
            );
            let requests = server.await.unwrap();
            assert_eq!(
                std::fs::read_to_string(output).unwrap(),
                format!("{literal}\ntwo words\n\n")
            );
            assert!(!forbidden.exists());
            assert_eq!(last_observation(&requests[1])["result"]["success"], true);
            assert_eq!(last_observation(&requests[2])["success"], false);
            assert_eq!(last_observation(&requests[3])["success"], false);
        });
    });
}

#[test]
fn raw_credentials_stay_in_private_rounds_and_private_stdin() {
    with_fixture("host-agent-secrets", |root, config| {
        save_keys(config, CredentialStorage::Plaintext);
        let program = helper(root, "private-input", "/bin/cat > \"$1\"\n/bin/cat \"$1\"");
        config.commands.whitelist = vec![program.display().to_string()];
        block_on(async {
            let private_file = root.join("private-input.json");
            let forbidden = root.join("secret-in-argv");
            let private = json!({"user":USER,"token":APP}).to_string();
            let (endpoint, server) = model(vec![
                Reply::Json(json!({"tool":"pushover_credentials","arguments":{}})),
                Reply::Json(json!({"tool":"command","arguments":{
                    "program":program,"arguments":[private_file],"stdin":private
                }})),
                command(&program, json!([forbidden, USER])),
                Reply::Failure(format!("provider failed with {USER} and {APP}")),
            ])
            .await;
            let raw = "@command read Pushover credentials and pass them through private stdin";
            let request = request(&endpoint, raw);
            let original = request
                .messages
                .iter()
                .map(|message| message.content.clone())
                .collect::<Vec<_>>();
            let outcome = run(config, raw, &request).await;
            let requests = server.await.unwrap();
            assert!(!requests[0].to_string().contains(USER));
            let keys = last_observation(&requests[1]);
            assert_eq!(keys["result"]["user_key"], USER);
            assert_eq!(keys["result"]["app_key"], APP);
            assert!(last_observation(&requests[2])["result"]["stdout"]
                .as_str()
                .unwrap()
                .contains(USER));
            assert_eq!(last_observation(&requests[3])["success"], false);
            assert!(!forbidden.exists(), "known keys cannot enter process argv");
            let persisted: Value =
                serde_json::from_str(&std::fs::read_to_string(private_file).unwrap()).unwrap();
            assert_eq!(persisted, json!({"user":USER,"token":APP}));
            let public = visible(&outcome);
            assert!(!public.contains(USER) && !public.contains(APP));
            assert!(public.contains("[REDACTED PUSHOVER KEY]"));
            assert!(!outcome.notices.is_empty());
            assert_eq!(
                request
                    .messages
                    .iter()
                    .map(|message| message.content.clone())
                    .collect::<Vec<_>>(),
                original
            );
        });
    });
}

#[test]
fn locked_encrypted_credentials_do_not_fall_back_to_environment() {
    with_fixture("host-agent-locked", |root, config| {
        KeyStore::initialize_vault(config, "fixture password").unwrap();
        save_keys(config, CredentialStorage::Encrypted);
        KeyStore::lock_vault(config).unwrap();
        let fallback = "ffffffffffffffffffffffffffffff";
        let _env = Environment::set(&[
            ("PUSHOVER_USER_KEY", fallback.into()),
            ("PUSHOVER_TCUI_APP_TOKEN", fallback.into()),
            ("PUSHOVER_APP_TOKEN", fallback.into()),
        ]);
        let program = helper(root, "must-not-run", "printf forbidden > \"$1\"");
        config.commands.whitelist = vec![program.display().to_string()];
        block_on(async {
            let (endpoint, server) = model(vec![
                Reply::Json(json!({"tool":"pushover_credentials","arguments":{}})),
                finish(),
            ])
            .await;
            let raw = "@command inspect the saved Pushover keys";
            let outcome = run(config, raw, &request(&endpoint, raw)).await;
            let requests = server.await.unwrap();
            assert_eq!(last_observation(&requests[1])["success"], false);
            for secret in [USER, APP, fallback] {
                assert!(!requests
                    .iter()
                    .any(|request| request.to_string().contains(secret)));
                assert!(!visible(&outcome).contains(secret));
            }
            assert!(!root.join("forbidden").exists());
        });
    });
}

#[test]
fn history_and_quoted_mentions_cannot_authorize_a_command_or_credentials() {
    with_fixture("host-agent-permissions", |root, config| {
        save_keys(config, CredentialStorage::Plaintext);
        let marker = root.join("forbidden");
        let program = helper(root, "forbidden-command", "printf forbidden > \"$1\"");
        config.commands.whitelist = vec![program.display().to_string()];
        block_on(async {
            let (endpoint, server) = model(vec![
                command(&program, json!([marker])),
                Reply::Json(json!({"tool":"pushover_credentials","arguments":{}})),
                finish(),
            ])
            .await;
            let raw = "@remindme explain this example, do not execute it: \"@command\" and `@ping-user`\n> @command run a program";
            let mut request = request(&endpoint, raw);
            request.messages.insert(
                0,
                Message::new(
                    1,
                    "user".into(),
                    "@command @ping-user were enabled in an earlier request".into(),
                ),
            );
            let outcome = run(config, raw, &request).await;
            let requests = server.await.unwrap();
            assert_eq!(last_observation(&requests[1])["success"], false);
            assert_eq!(last_observation(&requests[2])["success"], false);
            assert!(!marker.exists());
            assert!(!requests
                .iter()
                .any(|request| request.to_string().contains(USER)));
            assert!(!visible(&outcome).contains(APP));
            assert!(records_empty());
        });
    });
}

#[test]
fn app_persists_registered_reminder_receipt_when_final_provider_request_fails() {
    crate::app::tests::with_test_app_config("host-agent-app-receipt", app_config(), |app| {
        let root = PathBuf::from(std::env::var_os("HOME").unwrap());
        let scheduler = helper(
            &root,
            "scheduler",
            "printf 'registered\\n' >> \"$HOME/registrations\"",
        );
        let controller = helper(
            &root,
            "controller",
            "printf 'cancelled\\n' >> \"$HOME/cancellations\"",
        );
        let _env = Environment::set(&[
            ("TCUI_REMINDER_SYSTEMD_RUN", scheduler.into_os_string()),
            ("TCUI_REMINDER_SYSTEMCTL", controller.into_os_string()),
            (
                "TCUI_REMINDER_SYSTEMD_ANALYZE",
                OsString::from("/usr/bin/false"),
            ),
        ]);
        block_on(async {
            let (endpoint, server) = model(vec![
                Reply::Json(json!({"tool":"reminder","arguments":{
                    "action":"create","after_seconds":1200,"message":"inspect persisted subject"
                }})),
                finish(),
                Reply::Failure("final provider response deliberately unavailable".into()),
            ])
            .await;
            configure_app_provider(app, &endpoint);
            app.send_message("@remindme in twenty minutes to inspect persisted subject".into())
                .await
                .unwrap();
            let conversation = app.ui.tabs[app.ui.active_tab].active_conversation;
            let events = drain_app_request(app).await;
            let requests = server.await.unwrap();
            assert_eq!(
                requests.len(),
                3,
                "tool decision, finish, then ordinary final response"
            );
            let records = records();
            assert_eq!(records.len(), 1);
            let record = &records[0];
            assert_eq!(
                std::fs::read_to_string(root.join("registrations")).unwrap(),
                "registered\n"
            );
            let stored = app.storage.get_messages(conversation).unwrap();
            let receipt_index = stored
                .iter()
                .position(|message| {
                    message.content.contains(&record.id)
                        && message.content.contains("inspect persisted subject")
                })
                .expect(
                    "actual registered reminder receipt persists despite final provider failure",
                );
            let error_index = stored
                .iter()
                .position(|message| {
                    message.role == "assistant"
                        && message.content.contains("Provider request failed")
                })
                .expect("the ordinary final provider request really failed");
            assert!(
                receipt_index < error_index,
                "receipt precedes the failed assistant response"
            );
            assert!(events.contains("Provider request failed"));
            assert!(app.ui.tabs[app.ui.active_tab]
                .messages
                .iter()
                .any(|message| message.content.contains(&record.id)));
            let config = app.config.read().await.clone();
            let listed = crate::reminders::agent_tool(&config, &json!({"action":"list"})).unwrap();
            assert!(listed.contains(&record.id) && listed.contains("inspect persisted subject"));
            crate::reminders::agent_tool(&config, &json!({"action":"cancel","id":record.id}))
                .unwrap();
            assert!(records_empty());
            assert_eq!(
                std::fs::read_to_string(root.join("cancellations")).unwrap(),
                "cancelled\ncancelled\n"
            );
        });
    });
}

#[test]
fn app_redacts_keys_split_across_final_answer_and_thinking_sse_chunks() {
    crate::app::tests::with_test_app_config("host-agent-app-private-stream", app_config(), |app| {
        let mut config = app.config.blocking_read().clone();
        save_keys(&mut config, CredentialStorage::Plaintext);
        *app.config.blocking_write() = config;
        block_on(async {
            let (endpoint, server) = model(vec![
                Reply::Json(json!({"tool":"pushover_credentials","arguments":{}})),
                finish(),
                Reply::Deltas(vec![
                    json!({"content":format!("User: {}", &USER[..15]),"reasoning_content":format!("App: {}", &APP[..15])}),
                    json!({"content":format!("{}; app: {}", &USER[15..], &APP[..15]),"reasoning_content":format!("{}; user: {}", &APP[15..], &USER[..15])}),
                    json!({"content":format!("{}; complete.", &APP[15..]),"reasoning_content":format!("{}; complete.", &USER[15..])}),
                ]),
            ]).await;
            configure_app_provider(app, &endpoint);
            app.send_message("@command inspect the saved Pushover credentials, then explain what you observed without exposing keys".into()).await.unwrap();
            let conversation = app.ui.tabs[app.ui.active_tab].active_conversation;
            let events = drain_app_request(app).await;
            let requests = server.await.unwrap();
            assert_eq!(requests.len(), 3);
            assert_eq!(last_observation(&requests[1])["result"]["user_key"], USER);
            assert_eq!(last_observation(&requests[1])["result"]["app_key"], APP);
            for secret in [USER, APP] {
                assert!(
                    !requests[2].to_string().contains(secret),
                    "ordinary final provider context is redacted"
                );
                assert!(!events.contains(secret), "raw key reached an app/UI event");
            }
            let stored = app.storage.get_messages(conversation).unwrap();
            let answer = stored
                .iter()
                .rev()
                .find(|message| message.role == "assistant")
                .unwrap();
            assert!(answer.content.contains("[REDACTED PUSHOVER KEY]"));
            assert!(answer
                .thinking_content
                .as_deref()
                .unwrap()
                .contains("[REDACTED PUSHOVER KEY]"));
            let persisted = serde_json::to_string(&stored).unwrap();
            let displayed =
                serde_json::to_string(&app.ui.tabs[app.ui.active_tab].messages).unwrap();
            let log = std::fs::read_to_string(data_root().join("tcui.log")).unwrap();
            assert!(log.contains("provider.request") && log.contains("provider.response"));
            for secret in [USER, APP] {
                assert!(
                    !persisted.contains(secret),
                    "raw key persisted in conversation"
                );
                assert!(
                    !displayed.contains(secret),
                    "raw key reached displayed messages"
                );
                assert!(!log.contains(secret), "raw key reached diagnostics");
            }
        });
    });
}

fn app_config() -> AppConfig {
    let mut config = AppConfig {
        default_provider: "Ollama".into(),
        default_model: "host-agent-fixture".into(),
        providers: vec![crate::config::ProviderConfig {
            name: "Ollama".into(),
            endpoint: "http://127.0.0.1:9".into(),
            env_var: String::new(),
            backend_type: "openai".into(),
            auth_type: "api_key".into(),
        }],
        ..AppConfig::default()
    };
    config.notifications.desktop = false;
    config.notifications.pushover.enabled = false;
    config.web_search.enabled = false;
    #[cfg(feature = "memory")]
    {
        config.memory.enabled = false;
    }
    config
}

fn configure_app_provider(app: &mut crate::app::TuiApp, endpoint: &str) {
    let tab = &mut app.ui.tabs[app.ui.active_tab];
    tab.tab.provider = "Ollama".into();
    tab.tab.model = "host-agent-fixture".into();
    tab.tab.endpoint = Some(endpoint.into());
    app.terminal_has_focus
        .store(true, std::sync::atomic::Ordering::Relaxed);
}

async fn drain_app_request(app: &mut crate::app::TuiApp) -> String {
    use crate::app::action::Action;
    let id = app
        .active_request_id()
        .expect("real app submission registers a request");
    let mut events = String::new();
    let mut answer_chunks = String::new();
    let mut thinking_chunks = String::new();
    loop {
        let action = app
            .action_rx
            .recv()
            .await
            .expect("request completion event");
        events.push_str(&format!("{action:?}\n"));
        match &action {
            Action::StreamResponse(_, chunk) => answer_chunks.push_str(chunk),
            Action::StreamThinking(_, chunk) => thinking_chunks.push_str(chunk),
            _ => {}
        }
        let stopped = matches!(&action, Action::StopStream(stopped) if *stopped == id);
        app.dispatch(action).await.unwrap();
        if stopped {
            break;
        }
    }
    assert!(app.active_request_id().is_none());
    // Concatenate each channel too: checking individual chunks would miss split-key leaks.
    format!("{events}\n{answer_chunks}\n{thinking_chunks}")
}

fn with_fixture(label: &str, test: impl FnOnce(&Path, &mut AppConfig)) {
    let mut config = AppConfig::default();
    config.notifications.desktop = false;
    config.notifications.pushover.enabled = false;
    crate::app::tests::with_test_app_config(label, config.clone(), |_| {
        let root = PathBuf::from(std::env::var_os("HOME").unwrap());
        test(&root, &mut config);
    });
}

fn block_on(future: impl Future<Output = ()>) {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
        .block_on(async {
            tokio::time::timeout(DEADLINE, future)
                .await
                .expect("bounded local model/action test");
        });
}

fn helper(root: &Path, name: &str, body: &str) -> PathBuf {
    let path = root.join(name);
    std::fs::write(&path, format!("#!/bin/sh\nset -eu\n{body}\n")).unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o700)).unwrap();
    path
}

fn save_keys(config: &mut AppConfig, storage: CredentialStorage) {
    config.notifications.pushover.enabled = true;
    config.notifications.pushover.use_vault = true;
    for (id, key) in [(USER_KEY_ID, USER), (APP_KEY_ID, APP)] {
        KeyStore::set_pushover_secret(config, id, "Fixture", key, storage).unwrap();
    }
}

fn request(endpoint: &str, raw: &str) -> ChatRequest {
    ChatRequest {
        provider: crate::config::LOCAL_PROVIDER_NAME.into(),
        endpoint: endpoint.into(),
        model: "host-agent-fixture".into(),
        reasoning_effort: None,
        supported_reasoning_efforts: Vec::new(),
        backend_type: "openai".into(),
        api_key: None,
        system_prompt: "Fixture conversation instructions.".into(),
        messages: vec![Message::new(3, "user".into(), raw.into())],
    }
}

fn request_for_cancel(endpoint: &str, raw: &str, outcome: &AgentOutcome) -> ChatRequest {
    let mut request = request(endpoint, raw);
    request.messages.splice(0..0, outcome.messages.clone());
    request
}

fn visible(outcome: &AgentOutcome) -> String {
    outcome
        .messages
        .iter()
        .map(|message| message.content.as_str())
        .chain(outcome.notices.iter().map(String::as_str))
        .collect::<Vec<_>>()
        .join("\n")
}

fn data_root() -> PathBuf {
    PathBuf::from(std::env::var_os("XDG_DATA_HOME").unwrap()).join("tcui")
}

fn records() -> Vec<crate::reminders::ReminderRecord> {
    #[derive(serde::Deserialize)]
    struct Records {
        reminders: Vec<crate::reminders::ReminderRecord>,
    }
    toml::from_str::<Records>(&std::fs::read_to_string(data_root().join("reminders.toml")).unwrap())
        .unwrap()
        .reminders
}

fn records_empty() -> bool {
    !data_root().join("reminders.toml").exists() || records().is_empty()
}

struct Environment(Vec<(&'static str, Option<OsString>)>);

impl Environment {
    fn set(values: &[(&'static str, OsString)]) -> Self {
        let previous = values
            .iter()
            .map(|(name, _)| (*name, std::env::var_os(name)))
            .collect();
        for (name, value) in values {
            std::env::set_var(name, value);
        }
        Self(previous)
    }
}

impl Drop for Environment {
    fn drop(&mut self) {
        for (name, value) in &self.0 {
            match value {
                Some(value) => std::env::set_var(name, value),
                None => std::env::remove_var(name),
            }
        }
    }
}

enum Reply {
    Json(Value),
    Deltas(Vec<Value>),
    Failure(String),
}

fn finish() -> Reply {
    Reply::Json(json!({"tool":"finish","arguments":{"summary":"finished"}}))
}

fn command(program: &Path, arguments: Value) -> Reply {
    Reply::Json(json!({"tool":"command","arguments":{"program":program,"arguments":arguments}}))
}

async fn model(replies: Vec<Reply>) -> (String, tokio::task::JoinHandle<Vec<Value>>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let endpoint = format!("http://{}", listener.local_addr().unwrap());
    let server = tokio::spawn(async move {
        let mut requests = Vec::new();
        for reply in replies {
            let (mut stream, _) = listener.accept().await.unwrap();
            requests.push(read_http_json(&mut stream).await);
            let (status, content_type, body) = match reply {
                Reply::Json(value) => {
                    let event = json!({"choices":[{"delta":{"content":value.to_string()}}]});
                    (
                        "200 OK",
                        "text/event-stream",
                        format!("data: {event}\n\ndata: [DONE]\n\n"),
                    )
                }
                Reply::Deltas(deltas) => {
                    let mut body = String::new();
                    for delta in deltas {
                        let event = json!({"choices":[{"delta":delta}]});
                        body.push_str(&format!("data: {event}\n\n"));
                    }
                    body.push_str("data: [DONE]\n\n");
                    ("200 OK", "text/event-stream", body)
                }
                Reply::Failure(error) => (
                    "500 Internal Server Error",
                    "application/json",
                    json!({"error":{"message":error}}).to_string(),
                ),
            };
            let response = format!("HTTP/1.1 {status}\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len());
            stream.write_all(response.as_bytes()).await.unwrap();
            stream.shutdown().await.unwrap();
        }
        requests
    });
    (endpoint, server)
}

async fn read_http_json(stream: &mut TcpStream) -> Value {
    let mut bytes = Vec::new();
    let mut buffer = [0; 4096];
    let header_end = loop {
        if let Some(index) = bytes.windows(4).position(|window| window == b"\r\n\r\n") {
            break index + 4;
        }
        let count = stream.read(&mut buffer).await.unwrap();
        assert!(count > 0 && bytes.len() < 64 * 1024, "bounded HTTP header");
        bytes.extend_from_slice(&buffer[..count]);
    };
    let headers = std::str::from_utf8(&bytes[..header_end]).unwrap();
    assert!(headers.starts_with("POST /chat/completions HTTP/1.1\r\n"));
    let length = headers
        .lines()
        .find_map(|line| {
            let (name, value) = line.split_once(':')?;
            name.eq_ignore_ascii_case("content-length")
                .then(|| value.trim().parse::<usize>().unwrap())
        })
        .unwrap();
    assert!(length <= 1024 * 1024, "bounded private conversation");
    while bytes.len() < header_end + length {
        let count = stream.read(&mut buffer).await.unwrap();
        assert!(count > 0, "complete HTTP request body");
        bytes.extend_from_slice(&buffer[..count]);
    }
    serde_json::from_slice(&bytes[header_end..header_end + length]).unwrap()
}

fn last_observation(request: &Value) -> Value {
    let content = request["messages"].as_array().unwrap().last().unwrap()["content"]
        .as_str()
        .unwrap();
    // External-data envelopes deliberately surround, rather than alter, the JSON payload.
    let start = content.find('{').unwrap();
    let end = content.rfind('}').unwrap();
    serde_json::from_str(&content[start..=end]).unwrap()
}
