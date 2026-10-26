use std::ffi::OsString;
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;

use super::{handle_request, ping, store};
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
        let values = [
            ("HOME", root.join("home").into_os_string()),
            ("XDG_CONFIG_HOME", root.join("config").into_os_string()),
            ("XDG_DATA_HOME", root.join("data").into_os_string()),
            ("XDG_CACHE_HOME", root.join("cache").into_os_string()),
            ("TCUI_REMINDER_SYSTEMD_RUN", OsString::from("/usr/bin/true")),
            ("TCUI_REMINDER_SYSTEMCTL", OsString::from("/usr/bin/true")),
        ];
        let previous = values
            .iter()
            .map(|(key, _)| (*key, std::env::var_os(key)))
            .collect();
        for (key, value) in values {
            std::env::set_var(key, value);
        }
        let program = root.join("notify-ping");
        std::fs::write(&program, format!(
            "#!/bin/sh\n[ \"$PUSHOVER_USER_KEY\" = '{USER}' ] || exit 7\n[ \"$PUSHOVER_TCUI_APP_TOKEN\" = '{APP}' ] || exit 8\n[ -z \"$PUSHOVER_ENV_FILE\" ] || exit 9\n[ \"$1\" = --app ] && [ \"$2\" = tcui ] && [ \"$3\" = --title ] && [ \"$4\" = reminder ] && [ \"$5\" = -- ] || exit 10\nprintf 'delivered\\n' >> \"$HOME/delivery-events\"\n"
        )).unwrap();
        std::fs::set_permissions(&program, std::fs::Permissions::from_mode(0o700)).unwrap();
        let mut config = AppConfig::default();
        config.notifications.desktop = false;
        config.notifications.pushover.enabled = true;
        config.notifications.pushover.use_vault = true;
        config.notifications.pushover.command = program.to_string_lossy().into_owned();
        config.commands.whitelist = vec![config.notifications.pushover.command.clone()];
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
        handle_request(
            &self.config,
            "to look into subject X tomorrow at noon",
            false,
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
        .create_conversation_snapshot(1, "Protected chat", &[])
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
    runtime
        .block_on(ping::dispatch(&fixture.config, &record.id))
        .unwrap();
    assert_eq!(
        std::fs::read_to_string(fixture.events()).unwrap(),
        "delivered\n"
    );
    assert!(!fixture.pending(&record.id).exists());
    assert!(store::get(&record.id).unwrap().is_none());
    runtime
        .block_on(ping::dispatch(&fixture.config, &record.id))
        .unwrap();
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
    KeyStore::lock_vault(&fixture.config).unwrap();
    let runtime = tokio::runtime::Runtime::new().unwrap();
    assert!(runtime
        .block_on(ping::dispatch(&fixture.config, &record.id))
        .is_err());
    assert!(!fixture.events().exists());
    assert!(!fixture.pending(&record.id).exists());
    assert!(store::get(&record.id).unwrap().is_none());
}

#[test]
fn cancellation_removes_a_pending_plaintext_snapshot_before_delivery() {
    let _lock = crate::test_support::env_lock().lock().unwrap();
    let fixture = Fixture::new();
    let record = fixture.schedule();
    handle_request(&fixture.config, &format!("cancel {}", record.id), false).unwrap();
    assert!(!fixture.pending(&record.id).exists());
    assert!(store::get(&record.id).unwrap().is_none());
    assert!(!fixture.events().exists());
}

#[test]
fn scheduler_registration_failure_rolls_back_record_and_credential_snapshot() {
    let _lock = crate::test_support::env_lock().lock().unwrap();
    let fixture = Fixture::new();
    std::env::set_var("TCUI_REMINDER_SYSTEMD_RUN", "/usr/bin/false");
    assert!(handle_request(&fixture.config, "in 1h | Will not register", false).is_err());
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
    assert!(handle_request(&fixture.config, "in 1h | Must unlock", false).is_err());
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
    let worker =
        std::thread::spawn(move || handle_request(&config, "in 1h | Registration race", false));
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
    let cancelled = handle_request(&fixture.config, &format!("cancel {}", record.id), false);
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    runtime.block_on(async {
        let delivery = ping::dispatch(&fixture.config, &record.id);
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
    assert!(handle_request(&fixture.config, &format!("cancel {}", record.id), false).is_err());
    assert!(fixture.pending(&record.id).exists());
    assert!(store::get(&record.id).unwrap().is_some());
    assert!(!fixture.events().exists());
    std::env::set_var("TCUI_REMINDER_SYSTEMCTL", "/usr/bin/true");
    handle_request(&fixture.config, &format!("cancel {}", record.id), false).unwrap();
    assert!(!fixture.pending(&record.id).exists());
}

#[test]
fn explicitly_requested_pushover_never_degrades_to_a_local_only_timer() {
    let _lock = crate::test_support::env_lock().lock().unwrap();
    let mut fixture = Fixture::new();
    fixture.config.notifications.pushover.enabled = false;
    super::maybe_handle_request(
        &fixture.config,
        "@ping-user @remindme in 1h | Requires Pushover",
    )
    .expect("recognized reminder invocation");
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
