use std::ffi::OsString;
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;

use super::{agent_tool, ping, store};
use crate::config::key_store::SecretKind;
use crate::config::{AppConfig, KeyStore};
use crate::notifications::pushover::{APP_KEY_ID, USER_KEY_ID};

const USER: &str = "uuuuuuuuuuuuuuuuuuuuuuuuuuuuuu";
const APP: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

struct Fixture {
    root: PathBuf,
    previous: Vec<(&'static str, Option<OsString>)>,
    config: AppConfig,
}

impl Fixture {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!(
            "tcui-ping-lifecycle-{:032x}",
            rand::random::<u128>()
        ));
        std::fs::create_dir_all(root.join("home")).unwrap();
        let mut path = root.as_os_str().to_os_string();
        path.push(":");
        path.push(std::env::var_os("PATH").unwrap_or_default());
        let values = [
            ("HOME", root.join("home").into_os_string()),
            ("XDG_CONFIG_HOME", root.join("config").into_os_string()),
            ("XDG_DATA_HOME", root.join("data").into_os_string()),
            ("XDG_CACHE_HOME", root.join("cache").into_os_string()),
            ("TCUI_REMINDER_SYSTEMD_RUN", OsString::from("/usr/bin/true")),
            ("TCUI_REMINDER_SYSTEMCTL", OsString::from("/usr/bin/true")),
            (
                "TCUI_REMINDER_SYSTEMD_ANALYZE",
                OsString::from("/usr/bin/false"),
            ),
            ("PATH", path),
        ];
        let previous = values
            .iter()
            .map(|(key, _)| (*key, std::env::var_os(key)))
            .collect();
        for (key, value) in values {
            std::env::set_var(key, value);
        }
        let program = root.join("curl");
        std::fs::write(
            &program,
            format!(
                r#"#!/usr/bin/python3
import json, os, pathlib, sys
assert sys.argv[1] == "--disable"
assert not any(name.startswith("PUSHOVER_") for name in os.environ)
payload = json.load(sys.stdin)
assert payload["user"] == {USER:?} and payload["token"] == {APP:?}
assert payload["title"] == "reminder" and payload["message"]
assert payload["user"] not in repr(sys.argv) and payload["token"] not in repr(sys.argv)
with (pathlib.Path.home() / "delivery-events").open("a") as output:
    output.write("delivered\n")
print('{{"status":1}}')
"#
            ),
        )
        .unwrap();
        std::fs::set_permissions(&program, std::fs::Permissions::from_mode(0o700)).unwrap();
        let mut config = AppConfig::default();
        config.notifications.desktop = false;
        config.notifications.pushover.enabled = true;
        config.notifications.pushover.use_vault = true;
        config.commands.whitelist = vec![program.to_string_lossy().into_owned()];
        KeyStore::initialize_vault(&config, "fixture password").unwrap();
        KeyStore::set_secret(
            &config,
            USER_KEY_ID,
            "Pushover user",
            SecretKind::Password,
            USER,
        )
        .unwrap();
        KeyStore::set_secret(
            &config,
            APP_KEY_ID,
            "Pushover TCUI",
            SecretKind::Password,
            APP,
        )
        .unwrap();
        config.save().unwrap();
        Self {
            root,
            previous,
            config,
        }
    }

    fn pending(&self, id: &str) -> PathBuf {
        self.root
            .join("data/tcui/reminder-notifications")
            .join(format!("{id}.json"))
    }

    fn events(&self) -> PathBuf {
        self.root.join("home/delivery-events")
    }

    fn schedule(&self) -> super::ReminderRecord {
        agent_tool(
            &self.config,
            &serde_json::json!({
                "action": "create",
                "after_seconds": 1200,
                "message": "look into subject X",
                "pushover": true,
            }),
        )
        .unwrap();
        let records = store::list().unwrap();
        assert_eq!(records.len(), 1);
        records.into_iter().next().unwrap()
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = KeyStore::lock_vault(&self.config);
        for (key, previous) in self.previous.drain(..) {
            match previous {
                Some(value) => std::env::set_var(key, value),
                None => std::env::remove_var(key),
            }
        }
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

#[test]
fn encrypted_credentials_deliver_after_lock_then_one_shot_snapshot_is_consumed() {
    let _lock = crate::test_support::env_lock().lock().unwrap();
    let fixture = Fixture::new();
    crate::protection::configure(
        crate::protection::ProtectionPolicy {
            app_password: true,
            credentials: true,
            chats: true,
            ..Default::default()
        },
        "fixture password",
    )
    .unwrap();
    let storage = crate::storage::Storage::new().unwrap();
    storage
        .create_conversation_snapshot(1, "Protected chat", &[], None)
        .unwrap();
    assert!(!fixture.root.join("data/tcui/chat.key").exists());
    let record = fixture.schedule();
    assert!(record.pushover);
    assert!(!record.recurring);
    assert_eq!(
        std::fs::metadata(fixture.pending(&record.id))
            .unwrap()
            .permissions()
            .mode()
            & 0o777,
        0o600
    );
    KeyStore::lock_vault(&fixture.config).unwrap();
    assert!(KeyStore::get_secret(&fixture.config, USER_KEY_ID).is_err());
    let runtime = tokio::runtime::Runtime::new().unwrap();
    runtime.block_on(ping::dispatch(&record.id)).unwrap();
    assert_eq!(
        std::fs::read_to_string(fixture.events()).unwrap(),
        "delivered\n"
    );
    assert!(!fixture.pending(&record.id).exists());
    assert!(store::get(&record.id).unwrap().is_none());
    runtime.block_on(ping::dispatch(&record.id)).unwrap();
    assert_eq!(
        std::fs::read_to_string(fixture.events()).unwrap(),
        "delivered\n"
    );
}

#[test]
fn policy_revocation_blocks_and_consumes_a_one_shot_without_launching_the_helper() {
    let _lock = crate::test_support::env_lock().lock().unwrap();
    let mut fixture = Fixture::new();
    let record = fixture.schedule();
    fixture.config.commands.blacklist = fixture.config.commands.whitelist.clone();
    fixture.config.save().unwrap();
    KeyStore::lock_vault(&fixture.config).unwrap();
    let runtime = tokio::runtime::Runtime::new().unwrap();
    assert!(runtime.block_on(ping::dispatch(&record.id)).is_err());
    assert!(!fixture.events().exists());
    assert!(!fixture.pending(&record.id).exists());
    assert!(store::get(&record.id).unwrap().is_none());
}

#[test]
fn native_pushover_schedule_and_callback_enforce_the_full_argument_policy() {
    let _lock = crate::test_support::env_lock().lock().unwrap();
    let mut fixture = Fixture::new();
    fixture.config.commands.whitelist = vec!["curl -H".to_owned()];
    assert!(agent_tool(
        &fixture.config,
        &serde_json::json!({
            "action": "create", "after_seconds": 1200,
            "message": "Must not register unrelated argv", "pushover": true,
        }),
    )
    .is_err());
    assert!(store::list().unwrap().is_empty());
    assert!(!fixture.events().exists());

    fixture.config.commands.whitelist = vec!["curl --disable --silent".to_owned()];
    let record = fixture.schedule();
    assert!(fixture.pending(&record.id).exists());
    fixture.config.commands.blacklist = vec!["curl --disable --silent --fail".to_owned()];
    fixture.config.save().unwrap();
    KeyStore::lock_vault(&fixture.config).unwrap();
    let runtime = tokio::runtime::Runtime::new().unwrap();
    assert!(runtime.block_on(ping::dispatch(&record.id)).is_err());
    assert!(!fixture.events().exists());
    assert!(!fixture.pending(&record.id).exists());
    assert!(store::get(&record.id).unwrap().is_none());
}

#[test]
fn cancellation_removes_a_pending_plaintext_snapshot_before_delivery() {
    let _lock = crate::test_support::env_lock().lock().unwrap();
    let fixture = Fixture::new();
    let record = fixture.schedule();
    agent_tool(
        &fixture.config,
        &serde_json::json!({"action": "cancel", "id": record.id}),
    )
    .unwrap();
    assert!(!fixture.pending(&record.id).exists());
    assert!(store::get(&record.id).unwrap().is_none());
    assert!(!fixture.events().exists());
}

#[test]
fn scheduler_registration_failure_rolls_back_record_and_credential_snapshot() {
    let _lock = crate::test_support::env_lock().lock().unwrap();
    let fixture = Fixture::new();
    std::env::set_var("TCUI_REMINDER_SYSTEMD_RUN", "/usr/bin/false");
    assert!(agent_tool(
        &fixture.config,
        &serde_json::json!({
            "action": "create", "after_seconds": 3600, "message": "Will not register",
        }),
    )
    .is_err());
    assert!(store::list().unwrap().is_empty());
    let directory = fixture.root.join("data/tcui/reminder-notifications");
    assert!(std::fs::read_dir(directory).unwrap().all(|entry| entry
        .unwrap()
        .path()
        .extension()
        .is_none_or(|value| value != "json")));
    assert!(!fixture.events().exists());
}

#[test]
fn locked_vault_cannot_schedule_a_new_job_or_downgrade_to_environment_credentials() {
    let _lock = crate::test_support::env_lock().lock().unwrap();
    let fixture = Fixture::new();
    KeyStore::lock_vault(&fixture.config).unwrap();
    assert!(agent_tool(
        &fixture.config,
        &serde_json::json!({
            "action": "create", "after_seconds": 3600, "message": "Must unlock",
        }),
    )
    .is_err());
    assert!(store::list().unwrap().is_empty());
    assert!(!fixture
        .root
        .join("data/tcui/reminder-notifications")
        .exists());
}

#[test]
fn registration_serializes_dispatch_and_rejects_premature_cancellation() {
    use std::io::{Read, Write};
    use std::os::fd::AsRawFd;
    use std::os::unix::net::UnixListener;
    use std::time::Duration;

    let _lock = crate::test_support::env_lock().lock().unwrap();
    let fixture = Fixture::new();
    let socket = fixture.root.join("registration.sock");
    let listener = UnixListener::bind(&socket).unwrap();
    listener.set_nonblocking(true).unwrap();
    let scheduler = fixture.root.join("gated-scheduler");
    std::fs::write(&scheduler, format!(
        "#!/usr/bin/python3\nimport socket\ns=socket.socket(socket.AF_UNIX)\ns.settimeout(5)\ns.connect({:?})\ns.sendall(b'ready')\nassert s.recv(1)==b'x'\n",
        socket.to_string_lossy(),
    )).unwrap();
    std::fs::set_permissions(&scheduler, std::fs::Permissions::from_mode(0o700)).unwrap();
    std::env::set_var("TCUI_REMINDER_SYSTEMD_RUN", scheduler);
    let config = fixture.config.clone();
    let worker = std::thread::spawn(move || {
        agent_tool(
            &config,
            &serde_json::json!({
                "action": "create", "after_seconds": 3600, "message": "Registration race",
            }),
        )
    });
    let mut descriptor = libc::pollfd {
        fd: listener.as_raw_fd(),
        events: libc::POLLIN,
        revents: 0,
    };
    // SAFETY: the single pollfd refers to this live listener for the whole call.
    assert_eq!(unsafe { libc::poll(&mut descriptor, 1, 5_000) }, 1);
    assert_ne!(descriptor.revents & libc::POLLIN, 0);
    let (mut stream, _) = listener.accept().unwrap();
    stream
        .set_read_timeout(Some(Duration::from_secs(5)))
        .unwrap();
    stream
        .set_write_timeout(Some(Duration::from_secs(5)))
        .unwrap();
    let mut ready = [0; 5];
    stream.read_exact(&mut ready).unwrap();
    assert_eq!(&ready, b"ready");
    let record = store::list().unwrap().pop().unwrap();
    let cancelled = agent_tool(
        &fixture.config,
        &serde_json::json!({"action": "cancel", "id": record.id}),
    );
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    runtime.block_on(async {
        let delivery = ping::dispatch(&record.id);
        tokio::pin!(delivery);
        assert!(futures::poll!(delivery.as_mut()).is_pending());
        stream.write_all(b"x").unwrap();
        worker.join().unwrap().unwrap();
        assert!(
            cancelled.is_err(),
            "an in-flight registration must not report cancellation success"
        );
        delivery.await.unwrap();
    });
    assert!(!fixture.pending(&record.id).exists());
    assert!(store::get(&record.id).unwrap().is_none());
    assert_eq!(
        std::fs::read_to_string(fixture.events()).unwrap(),
        "delivered\n"
    );
}

#[test]
fn missing_timer_does_not_mask_a_service_stop_failure_or_delete_the_snapshot() {
    let _lock = crate::test_support::env_lock().lock().unwrap();
    let fixture = Fixture::new();
    let record = fixture.schedule();
    let controller = fixture.root.join("mixed-controller");
    std::fs::write(&controller, "#!/bin/sh\nfor unit do :; done\ncase \"$unit\" in\n *.timer) if [ \"$2\" = show ]; then printf 'not-found\\n'; exit 0; fi; printf 'Unit not loaded\\n' >&2; exit 1;;\n *.service) if [ \"$2\" = show ]; then printf 'loaded\\n'; exit 0; fi; printf 'Access denied\\n' >&2; exit 1;;\nesac\nexit 2\n").unwrap();
    std::fs::set_permissions(&controller, std::fs::Permissions::from_mode(0o700)).unwrap();
    std::env::set_var("TCUI_REMINDER_SYSTEMCTL", controller);
    assert!(agent_tool(
        &fixture.config,
        &serde_json::json!({"action": "cancel", "id": record.id}),
    )
    .is_err());
    assert!(fixture.pending(&record.id).exists());
    assert!(store::get(&record.id).unwrap().is_some());
    assert!(!fixture.events().exists());
    std::env::set_var("TCUI_REMINDER_SYSTEMCTL", "/usr/bin/true");
    agent_tool(
        &fixture.config,
        &serde_json::json!({"action": "cancel", "id": record.id}),
    )
    .unwrap();
    assert!(!fixture.pending(&record.id).exists());
}

#[test]
fn explicitly_requested_pushover_never_degrades_to_a_local_only_timer() {
    let _lock = crate::test_support::env_lock().lock().unwrap();
    let mut fixture = Fixture::new();
    fixture.config.notifications.pushover.enabled = false;
    assert!(agent_tool(
        &fixture.config,
        &serde_json::json!({
            "action": "create",
            "after_seconds": 3600,
            "message": "Requires Pushover",
            "pushover": true,
        }),
    )
    .is_err());
    assert!(store::list().unwrap().is_empty());
    assert!(!fixture
        .root
        .join("data/tcui/reminder-notifications")
        .exists());
}

#[test]
fn legacy_reminder_ciphertext_survives_separating_and_removing_the_chat_key() {
    let _lock = crate::test_support::env_lock().lock().unwrap();
    let fixture = Fixture::new();
    let mut record = fixture.schedule();
    record.pushover = false;
    store::upsert(record.clone()).unwrap();
    let root = fixture.root.join("data/tcui");
    // Reconstruct the pre-upgrade layout: enc:v1 records with only chat.key.
    std::fs::rename(root.join("reminder.key"), root.join("chat.key")).unwrap();
    assert_eq!(record.message_text().unwrap(), "look into subject X");
    std::fs::remove_file(root.join("chat.key")).unwrap();
    KeyStore::lock_vault(&fixture.config).unwrap();
    let restored = store::get(&record.id).unwrap().unwrap();
    assert_eq!(restored.message, record.message);
    assert_eq!(restored.message_text().unwrap(), "look into subject X");
}

#[test]
fn lost_reminder_keys_are_not_replaced_with_missing_or_incompatible_chat_keys() {
    let _lock = crate::test_support::env_lock().lock().unwrap();
    let fixture = Fixture::new();
    let record = fixture.schedule();
    let root = fixture.root.join("data/tcui");
    let path = root.join("reminder.key");
    let original = zeroize::Zeroizing::new(std::fs::read(&path).unwrap());
    std::fs::remove_file(&path).unwrap();
    assert!(record.message_text().is_err());
    assert!(!path.exists());
    crate::storage::crypto::SharedKey::create_at(&root.join("chat.key")).unwrap();
    assert!(record.message_text().is_err());
    assert!(!path.exists());
    std::fs::write(&path, &*original).unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o400)).unwrap();
    assert_eq!(record.message_text().unwrap(), "look into subject X");
    assert_eq!(
        store::get(&record.id).unwrap().unwrap().message,
        record.message
    );
}

#[test]
fn malformed_payload_is_consumed_after_the_failed_one_shot_attempt() {
    let _lock = crate::test_support::env_lock().lock().unwrap();
    let fixture = Fixture::new();
    let record = fixture.schedule();
    std::fs::write(fixture.pending(&record.id), b"invalid-json").unwrap();
    let runtime = tokio::runtime::Runtime::new().unwrap();
    assert!(runtime.block_on(ping::dispatch(&record.id)).is_err());
    assert!(!fixture.pending(&record.id).exists());
    assert!(store::get(&record.id).unwrap().is_none());
    assert!(!fixture.events().exists());
}

#[test]
fn invalid_configuration_does_not_leave_one_shot_keys_on_disk() {
    let _lock = crate::test_support::env_lock().lock().unwrap();
    let fixture = Fixture::new();
    let record = fixture.schedule();
    std::fs::write(fixture.root.join("config/tcui/config.toml"), b"[").unwrap();
    let runtime = tokio::runtime::Runtime::new().unwrap();
    assert!(runtime.block_on(ping::dispatch(&record.id)).is_err());
    assert!(!fixture.pending(&record.id).exists());
    assert!(store::get(&record.id).unwrap().is_none());
    assert!(!fixture.events().exists());
}

#[test]
fn rejected_delivery_still_consumes_one_shot_keys_and_registration() {
    let _lock = crate::test_support::env_lock().lock().unwrap();
    let fixture = Fixture::new();
    let record = fixture.schedule();
    std::fs::write(
        fixture.root.join("curl"),
        "#!/bin/sh\nprintf '{\"status\":0}'\n",
    )
    .unwrap();
    let runtime = tokio::runtime::Runtime::new().unwrap();
    assert!(runtime.block_on(ping::dispatch(&record.id)).is_err());
    assert!(!fixture.pending(&record.id).exists());
    assert!(store::get(&record.id).unwrap().is_none());
    assert!(!fixture.events().exists());
}

#[test]
fn post_stop_cleanup_needs_neither_configuration_nor_vault_unlock() {
    let _lock = crate::test_support::env_lock().lock().unwrap();
    let fixture = Fixture::new();
    let record = fixture.schedule();
    KeyStore::lock_vault(&fixture.config).unwrap();
    std::fs::write(fixture.root.join("config/tcui/config.toml"), b"[").unwrap();
    super::cleanup(&record.id).unwrap();
    assert!(!fixture.pending(&record.id).exists());
    assert!(store::get(&record.id).unwrap().is_none());
    assert!(!fixture.events().exists());
}

#[test]
fn recurring_jobs_keep_their_snapshot_until_explicit_cancellation() {
    let _lock = crate::test_support::env_lock().lock().unwrap();
    let fixture = Fixture::new();
    agent_tool(
        &fixture.config,
        &serde_json::json!({
            "action": "create", "daily": "09:00", "message": "Recurring fixture",
        }),
    )
    .unwrap();
    let record = store::list().unwrap().pop().unwrap();
    let runtime = tokio::runtime::Runtime::new().unwrap();
    runtime.block_on(ping::dispatch(&record.id)).unwrap();
    assert!(super::cleanup(&record.id).is_err());
    assert!(fixture.pending(&record.id).exists());
    assert!(store::get(&record.id).unwrap().is_some());
    agent_tool(
        &fixture.config,
        &serde_json::json!({"action": "cancel", "id": record.id}),
    )
    .unwrap();
    assert!(!fixture.pending(&record.id).exists());
    assert!(store::get(&record.id).unwrap().is_none());
}

#[test]
fn timer_cleanup_failure_removes_keys_but_keeps_retryable_registration() {
    let _lock = crate::test_support::env_lock().lock().unwrap();
    let fixture = Fixture::new();
    let record = fixture.schedule();
    std::env::set_var("TCUI_REMINDER_SYSTEMCTL", "/usr/bin/false");
    let runtime = tokio::runtime::Runtime::new().unwrap();
    assert!(runtime.block_on(ping::dispatch(&record.id)).is_err());
    assert!(!fixture.pending(&record.id).exists());
    assert!(store::get(&record.id).unwrap().is_some());
    std::env::set_var("TCUI_REMINDER_SYSTEMCTL", "/usr/bin/true");
    super::cleanup(&record.id).unwrap();
    assert!(store::get(&record.id).unwrap().is_none());
    assert_eq!(
        std::fs::read_to_string(fixture.events()).unwrap(),
        "delivered\n"
    );
}

#[test]
fn corrupt_or_missing_metadata_never_authorizes_deleting_recurring_payloads() {
    let _lock = crate::test_support::env_lock().lock().unwrap();
    let fixture = Fixture::new();
    agent_tool(
        &fixture.config,
        &serde_json::json!({
            "action": "create", "daily": "09:00", "message": "Retain recurring keys",
        }),
    )
    .unwrap();
    let record = store::list().unwrap().pop().unwrap();
    let metadata = fixture.root.join("data/tcui/reminders.toml");
    let original_metadata = std::fs::read(&metadata).unwrap();
    let original_payload =
        zeroize::Zeroizing::new(std::fs::read(fixture.pending(&record.id)).unwrap());
    let runtime = tokio::runtime::Runtime::new().unwrap();
    std::fs::write(&metadata, b"[").unwrap();
    assert!(super::cleanup(&record.id).is_err());
    assert!(runtime.block_on(ping::dispatch(&record.id)).is_err());
    assert_eq!(
        std::fs::read(fixture.pending(&record.id)).unwrap(),
        *original_payload
    );
    std::fs::remove_file(&metadata).unwrap();
    assert!(super::cleanup(&record.id).is_err());
    assert!(runtime.block_on(ping::dispatch(&record.id)).is_err());
    assert_eq!(
        std::fs::read(fixture.pending(&record.id)).unwrap(),
        *original_payload
    );
    std::fs::write(&metadata, original_metadata).unwrap();
    agent_tool(
        &fixture.config,
        &serde_json::json!({"action": "cancel", "id": record.id}),
    )
    .unwrap();
}

#[test]
fn malformed_orphan_payload_requires_cadence_recovery_before_cleanup() {
    let _lock = crate::test_support::env_lock().lock().unwrap();
    let fixture = Fixture::new();
    let record = fixture.schedule();
    store::remove(&record.id).unwrap();
    std::fs::write(fixture.pending(&record.id), b"unknown cadence").unwrap();
    assert!(super::cleanup(&record.id).is_err());
    let runtime = tokio::runtime::Runtime::new().unwrap();
    assert!(runtime.block_on(ping::dispatch(&record.id)).is_err());
    assert_eq!(
        std::fs::read(fixture.pending(&record.id)).unwrap(),
        b"unknown cadence"
    );
}

#[test]
fn typed_timing_choices_register_the_selected_backend_schedule() {
    let _lock = crate::test_support::env_lock().lock().unwrap();
    let fixture = Fixture::new();
    let registrations = fixture.root.join("registrations.jsonl");
    let scheduler = fixture.root.join("record-scheduler");
    std::fs::write(
        &scheduler,
        format!(
            "#!/usr/bin/python3\nimport json, pathlib, sys\nwith pathlib.Path({:?}).open('a') as out:\n    out.write(json.dumps(sys.argv[1:]) + '\\n')\n",
            registrations.to_str().unwrap(),
        ),
    )
    .unwrap();
    std::fs::set_permissions(&scheduler, std::fs::Permissions::from_mode(0o700)).unwrap();
    std::env::set_var("TCUI_REMINDER_SYSTEMD_RUN", scheduler);
    let analyzer = fixture.root.join("calendar-analyzer");
    std::fs::write(
        &analyzer,
        "#!/bin/sh\n[ \"$LC_ALL\" = C ] || exit 1\n[ \"$1\" = calendar ] || exit 1\n[ \"$2\" = --iterations=1 ] || exit 1\n[ \"$3\" = -- ] || exit 1\nprintf '  Next elapse: Thu 2099-01-01 00:00:00 UTC\\n'\n",
    )
    .unwrap();
    std::fs::set_permissions(&analyzer, std::fs::Permissions::from_mode(0o700)).unwrap();
    std::env::set_var("TCUI_REMINDER_SYSTEMD_ANALYZE", analyzer);
    let at = chrono::Local::now() + chrono::Duration::hours(2);
    let choices = [
        (
            serde_json::json!({"after_seconds": 1200}),
            "--on-active",
            "1200s".to_owned(),
            false,
        ),
        (
            serde_json::json!({"at": at.to_rfc3339()}),
            "--on-calendar",
            at.with_timezone(&chrono::Utc)
                .format("%Y-%m-%d %H:%M:%S UTC")
                .to_string(),
            false,
        ),
        (
            serde_json::json!({"daily": "09:00"}),
            "--on-calendar",
            "*-*-* 09:00:00".to_owned(),
            true,
        ),
        (
            serde_json::json!({"weekly": {"day": "mon", "time": "09:30:15"}}),
            "--on-calendar",
            "Mon *-*-* 09:30:15".to_owned(),
            true,
        ),
        (
            serde_json::json!({"calendar": "hourly"}),
            "--on-calendar",
            "hourly".to_owned(),
            true,
        ),
    ];
    for (timing, flag, value, recurring) in choices {
        let mut arguments = serde_json::json!({
            "action": "create", "message": "Do X", "pushover": true,
        });
        arguments
            .as_object_mut()
            .unwrap()
            .extend(timing.as_object().unwrap().clone());
        let response = agent_tool(&fixture.config, &arguments).unwrap();
        let record = store::list().unwrap().pop().unwrap();
        assert!(response.contains(&record.id));
        assert_eq!(record.recurring, recurring);
        let calls = std::fs::read_to_string(&registrations).unwrap();
        let last: Vec<String> = serde_json::from_str(calls.lines().last().unwrap()).unwrap();
        assert!(last
            .windows(2)
            .any(|args| args[0] == flag && args[1] == value));
        assert!(last
            .windows(2)
            .any(|args| { args[0] == "reminder-ping-dispatch" && args[1] == record.id }));
        assert_eq!(
            last.iter()
                .any(|arg| arg.starts_with("--property=ExecStopPost=")),
            !recurring,
        );
        agent_tool(
            &fixture.config,
            &serde_json::json!({"action": "cancel", "id": record.id}),
        )
        .unwrap();
        assert!(!fixture.pending(&record.id).exists());
        assert!(store::list().unwrap().is_empty());
    }
    assert_eq!(
        std::fs::read_to_string(registrations)
            .unwrap()
            .lines()
            .count(),
        5,
    );
}

#[test]
fn invalid_tool_arguments_fail_before_registration_or_snapshot_side_effects() {
    let _lock = crate::test_support::env_lock().lock().unwrap();
    let fixture = Fixture::new();
    let invalid = [
        serde_json::json!({"after_seconds": 0}),
        serde_json::json!({"after_seconds": -1}),
        serde_json::json!({"after_seconds": 1.5}),
        serde_json::json!({"after_seconds": u64::MAX}),
        serde_json::json!({"after_seconds": null}),
        serde_json::json!({"after_seconds": 10, "at": null}),
        serde_json::json!({"after_seconds": 10, "daily": "09:00"}),
        serde_json::json!({"at": "2020-01-01T12:00:00Z"}),
        serde_json::json!({"at": "9999-01-01T12:00:00Z"}),
        serde_json::json!({"at": "2099-01-01 12:00"}),
        serde_json::json!({"daily": "24:00"}),
        serde_json::json!({"daily": "9:00"}),
        serde_json::json!({"daily": "23:59:60"}),
        serde_json::json!({"weekly": {"day": "monday", "time": "09:00"}}),
        serde_json::json!({"weekly": {"day": "mon", "time": "09:00", "extra": true}}),
        serde_json::json!({"calendar": ""}),
        serde_json::json!({"calendar": "hourly\nextra"}),
        serde_json::json!({"calendar": "not a calendar"}),
        serde_json::json!({"after_seconds": 10, "extra": true}),
        serde_json::json!({}),
    ];
    for timing in invalid {
        let mut arguments = serde_json::json!({"action": "create", "message": "Do X"});
        arguments
            .as_object_mut()
            .unwrap()
            .extend(timing.as_object().unwrap().clone());
        assert!(
            agent_tool(&fixture.config, &arguments).is_err(),
            "{arguments}"
        );
    }
    for arguments in [
        serde_json::json!({"action": "list", "message": "unexpected"}),
        serde_json::json!({"action": "cancel", "id": "../escape"}),
        serde_json::json!({"action": "cancel", "id": ""}),
        serde_json::json!({"action": "cancel", "id": "abc123", "extra": true}),
        serde_json::json!({"action": "create", "after_seconds": 10, "message": " "}),
        serde_json::json!({"action": "create", "after_seconds": 10, "message": "\0"}),
        serde_json::json!({"action": "create", "after_seconds": 10, "message": "x".repeat(1025)}),
    ] {
        assert!(
            agent_tool(&fixture.config, &arguments).is_err(),
            "{arguments}"
        );
    }
    for name in ["reminders.toml", "reminder.key", "reminder-notifications"] {
        assert!(
            !fixture.root.join("data/tcui").join(name).exists(),
            "{name}"
        );
    }
    assert!(!fixture.events().exists());
}

#[test]
fn calendar_without_a_future_occurrence_is_rejected_before_persisting_keys() {
    let _lock = crate::test_support::env_lock().lock().unwrap();
    let fixture = Fixture::new();
    let analyzer = fixture.root.join("expired-calendar");
    std::fs::write(&analyzer, "#!/bin/sh\nprintf '  Next elapse: never\\n'\n").unwrap();
    std::fs::set_permissions(&analyzer, std::fs::Permissions::from_mode(0o700)).unwrap();
    std::env::set_var("TCUI_REMINDER_SYSTEMD_ANALYZE", analyzer);
    assert!(agent_tool(
        &fixture.config,
        &serde_json::json!({
            "action": "create", "calendar": "2020-01-01", "message": "Too late",
        }),
    )
    .is_err());
    assert!(!fixture.root.join("data/tcui/reminders.toml").exists());
    assert!(!fixture
        .root
        .join("data/tcui/reminder-notifications")
        .exists());
}

struct SchedulerGate {
    listener: std::os::unix::net::UnixListener,
}

impl SchedulerGate {
    fn new(fixture: &Fixture, variable: &str, before: &str, after: &str) -> Self {
        let socket = fixture.root.join("scheduler-gate.sock");
        let listener = std::os::unix::net::UnixListener::bind(&socket).unwrap();
        listener.set_nonblocking(true).unwrap();
        let program = fixture.root.join("gated-native-scheduler");
        std::fs::write(
            &program,
            format!(
                "#!/usr/bin/python3\nimport os, pathlib, socket, sys\n{before}\ns=socket.socket(socket.AF_UNIX)\ns.settimeout(5)\ns.connect({:?})\ns.sendall(str(os.getpid()).encode()+b'\\n')\nassert s.recv(1)==b'x'\ns.close()\n{after}\n",
                socket.to_str().unwrap(),
            ),
        )
        .unwrap();
        std::fs::set_permissions(&program, std::fs::Permissions::from_mode(0o700)).unwrap();
        std::env::set_var(variable, program);
        Self { listener }
    }

    fn accept(&self) -> (std::os::unix::net::UnixStream, libc::pid_t) {
        use std::io::Read;
        use std::os::fd::AsRawFd;

        let mut descriptor = libc::pollfd {
            fd: self.listener.as_raw_fd(),
            events: libc::POLLIN,
            revents: 0,
        };
        // SAFETY: this pollfd refers to the live fixture listener.
        assert_eq!(unsafe { libc::poll(&mut descriptor, 1, 5_000) }, 1);
        let (mut stream, _) = self.listener.accept().unwrap();
        stream
            .set_read_timeout(Some(std::time::Duration::from_secs(5)))
            .unwrap();
        stream
            .set_write_timeout(Some(std::time::Duration::from_secs(5)))
            .unwrap();
        let mut pid = Vec::new();
        loop {
            let mut byte = [0];
            stream.read_exact(&mut byte).unwrap();
            if byte[0] == b'\n' {
                break;
            }
            pid.push(byte[0]);
        }
        (stream, String::from_utf8(pid).unwrap().parse().unwrap())
    }

    fn release(mut stream: std::os::unix::net::UnixStream) {
        use std::io::Write;
        // Cancellation may already have killed and reaped the gated child.
        let _ = stream.write_all(b"x");
    }
}

fn write_scheduler_program(fixture: &Fixture, variable: &str, name: &str, body: &str) {
    let program = fixture.root.join(name);
    std::fs::write(
        &program,
        format!("#!/usr/bin/python3\nimport pathlib, sys\n{body}\n"),
    )
    .unwrap();
    std::fs::set_permissions(&program, std::fs::Permissions::from_mode(0o700)).unwrap();
    std::env::set_var(variable, program);
}

fn assert_no_pending_registration(fixture: &Fixture) {
    assert!(store::list().unwrap().is_empty());
    let directory = fixture.root.join("data/tcui/reminder-notifications");
    if directory.exists() {
        assert!(std::fs::read_dir(directory).unwrap().all(|entry| {
            entry
                .unwrap()
                .path()
                .extension()
                .is_none_or(|extension| extension != "json")
        }));
    }
    assert!(!fixture.events().exists());
}

fn assert_scheduler_reaped(pid: libc::pid_t) {
    let mut status = 0;
    // SAFETY: these calls inspect the fixture's native child without signaling it.
    assert_eq!(
        unsafe { libc::waitpid(pid, &mut status, libc::WNOHANG) },
        -1
    );
    assert_eq!(
        std::io::Error::last_os_error().raw_os_error(),
        Some(libc::ECHILD)
    );
    // SAFETY: signal 0 only checks whether this process still exists.
    assert_eq!(unsafe { libc::kill(pid, 0) }, -1);
    assert_eq!(
        std::io::Error::last_os_error().raw_os_error(),
        Some(libc::ESRCH)
    );
}

#[test]
fn cancelled_calendar_preflight_cannot_register_a_timer_or_persist_credentials() {
    let _lock = crate::test_support::env_lock().lock().unwrap();
    let fixture = Fixture::new();
    let registration = fixture.root.join("timer-registered");
    write_scheduler_program(
        &fixture,
        "TCUI_REMINDER_SYSTEMD_RUN",
        "record-registration",
        &format!("pathlib.Path({:?}).touch()", registration.to_str().unwrap()),
    );
    let gate = SchedulerGate::new(
        &fixture,
        "TCUI_REMINDER_SYSTEMD_ANALYZE",
        "",
        "print('  Next elapse: Thu 2099-01-01 00:00:00 UTC')",
    );
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    let pid = runtime.block_on(async {
        let mut request = Box::pin(super::agent_tool_cancellable(
            fixture.config.clone(),
            serde_json::json!({
                "action": "create", "calendar": "hourly", "message": "Cancelled preflight",
                "pushover": true,
            }),
        ));
        assert!(futures::poll!(request.as_mut()).is_pending());
        let (stream, pid) = gate.accept();
        drop(request);
        SchedulerGate::release(stream);
        pid
    });
    // Runtime shutdown joins the cancelled blocking worker, not just its future.
    drop(runtime);
    assert_scheduler_reaped(pid);
    assert!(!registration.exists());
    assert_no_pending_registration(&fixture);
}

#[test]
fn cancelled_registration_stops_the_timer_and_removes_its_private_payload() {
    let _lock = crate::test_support::env_lock().lock().unwrap();
    let fixture = Fixture::new();
    let timer = fixture.root.join("active-timer");
    let stops = fixture.root.join("stopped-units");
    write_scheduler_program(
        &fixture,
        "TCUI_REMINDER_SYSTEMCTL",
        "rollback-controller",
        &format!(
            "assert sys.argv[2]=='stop'\npathlib.Path({:?}).unlink(missing_ok=True)\nwith pathlib.Path({:?}).open('a') as out: out.write(sys.argv[-1]+'\\n')",
            timer.to_str().unwrap(), stops.to_str().unwrap(),
        ),
    );
    let gate = SchedulerGate::new(
        &fixture,
        "TCUI_REMINDER_SYSTEMD_RUN",
        &format!("pathlib.Path({:?}).touch()", timer.to_str().unwrap()),
        "",
    );
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    let pid = runtime.block_on(async {
        let mut request = Box::pin(super::agent_tool_cancellable(
            fixture.config.clone(),
            serde_json::json!({
                "action": "create", "after_seconds": 1200, "message": "Cancelled registration",
                "pushover": true,
            }),
        ));
        assert!(futures::poll!(request.as_mut()).is_pending());
        let (stream, pid) = gate.accept();
        let record = store::list().unwrap().pop().unwrap();
        assert!(fixture.pending(&record.id).exists() && timer.exists());
        drop(request);
        SchedulerGate::release(stream);
        pid
    });
    drop(runtime);
    assert_scheduler_reaped(pid);
    assert!(!timer.exists());
    let stopped = std::fs::read_to_string(stops).unwrap();
    assert!(stopped.contains(".timer\n") && stopped.contains(".service\n"));
    assert_no_pending_registration(&fixture);
}

#[test]
fn completed_but_unobserved_registration_rolls_back_before_releasing_its_guard() {
    let _lock = crate::test_support::env_lock().lock().unwrap();
    let fixture = Fixture::new();
    let timer = fixture.root.join("active-timer");
    write_scheduler_program(
        &fixture,
        "TCUI_REMINDER_SYSTEMCTL",
        "rollback-controller",
        &format!(
            "pathlib.Path({:?}).unlink(missing_ok=True)",
            timer.to_str().unwrap()
        ),
    );
    let gate = SchedulerGate::new(
        &fixture,
        "TCUI_REMINDER_SYSTEMD_RUN",
        "",
        &format!("pathlib.Path({:?}).touch()", timer.to_str().unwrap()),
    );
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    runtime.block_on(async {
        let (owner, result, worker) = super::execution::start(
            fixture.config.clone(),
            serde_json::json!({
                "action": "create", "after_seconds": 1200, "message": "Unobserved result",
                "pushover": true,
            }),
        );
        let (stream, pid) = gate.accept();
        SchedulerGate::release(stream);
        let receipt = result.await.unwrap().unwrap();
        let record = store::list().unwrap().pop().unwrap();
        assert!(receipt.contains(&record.id));
        assert!(timer.exists() && fixture.pending(&record.id).exists());
        assert!(ping::JobGuard::open(&record.id).is_err());
        // The channel has a completed result, but the requesting future has not
        // acknowledged it. Dropping that owner must not commit the registration.
        drop(owner);
        worker.await.unwrap();
        assert_scheduler_reaped(pid);
        assert!(ping::JobGuard::open(&record.id).is_ok());
    });
    assert!(!timer.exists());
    assert_no_pending_registration(&fixture);
}

#[test]
fn unobserved_registration_cleanup_failure_retains_metadata_but_not_credentials() {
    let _lock = crate::test_support::env_lock().lock().unwrap();
    let fixture = Fixture::new();
    std::env::set_var("TCUI_REMINDER_SYSTEMCTL", "/usr/bin/false");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    runtime.block_on(async {
        let (owner, result, worker) = super::execution::start(
            fixture.config.clone(),
            serde_json::json!({
                "action": "create", "after_seconds": 1200, "message": "Recover cleanup",
                "pushover": true,
            }),
        );
        result.await.unwrap().unwrap();
        let record = store::list().unwrap().pop().unwrap();
        drop(owner);
        worker.await.unwrap();
        assert!(store::get(&record.id).unwrap().is_some());
        assert!(!fixture.pending(&record.id).exists());
        std::env::set_var("TCUI_REMINDER_SYSTEMCTL", "/usr/bin/true");
        agent_tool(
            &fixture.config,
            &serde_json::json!({"action": "cancel", "id": record.id}),
        )
        .unwrap();
    });
    assert_no_pending_registration(&fixture);
}

#[test]
fn observed_async_receipts_commit_one_shot_and_recurring_reminders_and_release_the_guard() {
    let _lock = crate::test_support::env_lock().lock().unwrap();
    let fixture = Fixture::new();
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    runtime.block_on(async {
        for timing in [
            serde_json::json!({"after_seconds": 1200}),
            serde_json::json!({"daily": "09:00"}),
        ] {
            let mut arguments = serde_json::json!({
                "action": "create", "message": "Observed registration", "pushover": true,
            });
            arguments
                .as_object_mut()
                .unwrap()
                .extend(timing.as_object().unwrap().clone());
            let receipt = super::agent_tool_cancellable(fixture.config.clone(), arguments)
                .await
                .unwrap();
            let record = store::list().unwrap().pop().unwrap();
            assert!(receipt.contains(&record.id));
            assert!(fixture.pending(&record.id).exists());
            // A returned receipt must release the registration lock immediately,
            // even before the blocking worker receives its acknowledgement.
            agent_tool(
                &fixture.config,
                &serde_json::json!({"action": "cancel", "id": record.id}),
            )
            .unwrap();
            assert_no_pending_registration(&fixture);
        }
    });
}

#[test]
fn cancelled_cancel_action_kills_its_child_and_keeps_state_for_explicit_recovery() {
    let _lock = crate::test_support::env_lock().lock().unwrap();
    let fixture = Fixture::new();
    let record = fixture.schedule();
    let stopped = fixture.root.join("stop-finished");
    let gate = SchedulerGate::new(
        &fixture,
        "TCUI_REMINDER_SYSTEMCTL",
        "",
        &format!("pathlib.Path({:?}).touch()", stopped.to_str().unwrap()),
    );
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    runtime.block_on(async {
        let (owner, result, worker) = super::execution::start(
            fixture.config.clone(),
            serde_json::json!({"action": "cancel", "id": record.id}),
        );
        let (stream, pid) = gate.accept();
        drop(owner);
        drop(result);
        // Do not release the fake controller until the worker has stopped and
        // reaped it. Its socket timeout is five seconds, longer than this bound.
        tokio::time::timeout(std::time::Duration::from_secs(2), worker)
            .await
            .unwrap()
            .unwrap();
        SchedulerGate::release(stream);
        assert_scheduler_reaped(pid);
    });
    assert!(!stopped.exists());
    assert!(store::get(&record.id).unwrap().is_some());
    assert!(fixture.pending(&record.id).exists());
    std::env::set_var("TCUI_REMINDER_SYSTEMCTL", "/usr/bin/true");
    agent_tool(
        &fixture.config,
        &serde_json::json!({"action": "cancel", "id": record.id}),
    )
    .unwrap();
    assert_no_pending_registration(&fixture);
}
