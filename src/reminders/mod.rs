mod parse;
mod ping;
#[cfg(all(test, unix))]
mod ping_tests;
mod store;
mod systemd;

use crate::storage::crypto::{decrypt_shared_text_with_key, encrypt_shared_text_with_key};
use chrono::{DateTime, Local, NaiveTime, Weekday};
use serde::{Deserialize, Serialize};

use crate::config::AppConfig;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ReminderSchedule {
    After(std::time::Duration),
    At(DateTime<Local>),
    Daily(NaiveTime),
    Weekly(Weekday, NaiveTime),
    Calendar(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ReminderRequest {
    pub(crate) schedule: ReminderSchedule,
    pub(crate) message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub(crate) struct ReminderRecord {
    pub(crate) id: String,
    pub(crate) display_schedule: String,
    pub(crate) recurring: bool,
    pub(crate) message: String,
    pub(crate) unit: String,
    pub(crate) created_at: String,
    #[serde(default)]
    pub(crate) pushover: bool,
}

impl ReminderRecord {
    fn new(request: &ReminderRequest, unit: String) -> Result<Self, ReminderError> {
        let id = format!("{:016x}", rand::random::<u64>());
        Ok(Self {
            id,
            display_schedule: parse::describe_schedule(&request.schedule),
            recurring: request.schedule.is_recurring(),
            message: encrypt_shared_text_with_key(&store::message_key()?, &request.message)
                .map_err(color_eyre::Report::new)?,
            unit,
            created_at: Local::now().to_rfc3339(),
            pushover: false,
        })
    }

    #[cfg(test)]
    pub(crate) fn new_test(
        id: &str,
        schedule: ReminderSchedule,
        message: &str,
        created_at: DateTime<Local>,
    ) -> Self {
        Self {
            id: id.to_string(),
            display_schedule: parse::describe_schedule(&schedule),
            recurring: schedule.is_recurring(),
            message: encrypt_shared_text_with_key(
                &store::message_key().expect("reminder key"),
                message,
            )
            .expect("encrypt reminder"),
            unit: format!("tcui-reminder-{id}"),
            created_at: created_at.to_rfc3339(),
            pushover: false,
        }
    }

    fn message_text(&self) -> Result<String, ReminderError> {
        Ok(
            decrypt_shared_text_with_key(&store::message_key()?, &self.message)
                .map_err(color_eyre::Report::new)?,
        )
    }
}

impl ReminderSchedule {
    fn is_recurring(&self) -> bool {
        !matches!(self, Self::After(_) | Self::At(_))
    }
}

#[derive(Debug, thiserror::Error)]
pub(crate) enum ReminderError {
    #[error("{0}")]
    Invalid(String),
    #[error("failed to read reminder store: {0}")]
    Io(#[from] std::io::Error),
    #[error("failed to read reminder metadata: {0}")]
    TomlDe(#[from] toml::de::Error),
    #[error("failed to write reminder metadata: {0}")]
    TomlSer(#[from] toml::ser::Error),
    #[error("failed to access local reminder secret: {0}")]
    Crypto(#[from] color_eyre::Report),
    #[error("{0}")]
    Schedule(String),
    #[error("Pushover reminder unavailable: {0}")]
    Notification(color_eyre::Report),
}

struct ReminderCommand {
    input: String,
    pushover: bool,
}

pub(crate) fn maybe_handle_request(config: &AppConfig, request: &str) -> Option<String> {
    let command = extract_request(request)?;
    Some(
        match handle_request(config, &command.input, command.pushover) {
            Ok(response) => response,
            Err(error) => format!("Reminder scheduling failed: {error}"),
        },
    )
}

pub(crate) async fn dispatch(config: &AppConfig, id: &str) -> color_eyre::Result<()> {
    let _guard = ping::JobGuard::wait(id).await?;
    let Some(record) = store::get(id)? else {
        return Ok(());
    };
    let message = record.message_text()?;
    // External delivery is best-effort; keep the in-app/journal notice and
    // one-shot bookkeeping even when a configured notification backend fails.
    let _ = crate::notifications::notify_reminder(config, &message).await;
    println!("tcui reminder: {message}");
    if !record.recurring {
        store::remove(id)?;
    }
    Ok(())
}

pub(crate) async fn dispatch_ping(config: &AppConfig, id: &str) -> color_eyre::Result<()> {
    ping::dispatch(config, id).await
}

fn handle_request(
    config: &AppConfig,
    input: &str,
    pushover: bool,
) -> Result<String, ReminderError> {
    if input.eq_ignore_ascii_case("list") {
        return list_reminders();
    }
    if let Some(id) = input.strip_prefix("forget ") {
        return forget_reminder(id.trim());
    }
    if let Some(id) = input.strip_prefix("cancel ") {
        return forget_reminder(id.trim());
    }
    if pushover && !config.notifications.pushover.enabled {
        return Err(ReminderError::Invalid(
            "Pushover is disabled. Configure and enable it before scheduling a Pushover reminder."
                .to_owned(),
        ));
    }
    schedule_request(config, input)
}

fn schedule_request(config: &AppConfig, input: &str) -> Result<String, ReminderError> {
    let request = parse::parse_request(input)?;
    let delivery =
        crate::notifications::pushover::prepare(config).map_err(ReminderError::Notification)?;
    let mut record = ReminderRecord::new(&request, String::new())?;
    record.unit = format!("tcui-reminder-{}", record.id);
    record.pushover = delivery.is_some();
    let guard = ping::JobGuard::open(&record.id)
        .map_err(|error| ReminderError::Schedule(format!("Cannot lock reminder state: {error}")))?;
    store::upsert(record.clone())?;
    if let Some(delivery) = delivery {
        if let Err(error) = ping::save(&guard, delivery, &request.message, record.recurring) {
            store::remove(&record.id)?;
            return Err(ReminderError::Notification(error));
        }
    }
    if let Err(error) = systemd::schedule(&record.id, &request.schedule, record.pushover) {
        if let Err(cleanup) = systemd::cancel(&record.unit) {
            return Err(ReminderError::Schedule(format!(
                "{error}; timer cleanup failed: {cleanup}. Cancel reminder {} before retrying",
                record.id,
            )));
        }
        if record.pushover {
            ping::remove(&guard).map_err(ReminderError::Notification)?;
        }
        store::remove(&record.id)?;
        return Err(error);
    }
    Ok(format!(
        "Scheduled {} reminder {} (ID {}): {}{}",
        if request.schedule.is_recurring() {
            "recurring"
        } else {
            "one-shot"
        },
        parse::describe_schedule(&request.schedule),
        record.id,
        request.message,
        if record.pushover {
            "\nPushover jobs keep plaintext messages and credentials in private files. One-shot files are removed after the timer attempts delivery; recurring files remain until cancellation."
        } else {
            ""
        },
    ))
}

fn list_reminders() -> Result<String, ReminderError> {
    let reminders = store::list()?;
    if reminders.is_empty() {
        return Ok("No reminders scheduled.".to_string());
    }
    let mut lines = vec!["Scheduled reminders:".to_string()];
    for reminder in reminders {
        let message = reminder.message_text()?;
        let cadence = if reminder.recurring {
            "recurring"
        } else {
            "one-shot"
        };
        lines.push(format!(
            "- {} [{}] {}: {}",
            reminder.id, cadence, reminder.display_schedule, message
        ));
    }
    Ok(lines.join("\n"))
}

fn forget_reminder(id: &str) -> Result<String, ReminderError> {
    if id.is_empty() {
        return Err(ReminderError::Invalid(
            "use `forget <reminder-id>`".to_string(),
        ));
    }
    let guard = ping::JobGuard::open(id)
        .map_err(|error| ReminderError::Schedule(format!("Cannot lock reminder state: {error}")))?;
    let Some(record) = store::get(id)? else {
        return Err(ReminderError::Invalid(format!(
            "reminder `{id}` was not found"
        )));
    };
    systemd::cancel(&record.unit)?;
    if record.pushover {
        ping::remove(&guard).map_err(ReminderError::Notification)?;
    }
    store::remove(id)?;
    Ok(format!("Forgot reminder {id}."))
}

fn extract_request(request: &str) -> Option<ReminderCommand> {
    let mut remaining = request.trim_start();
    let mut scheduling = false;
    let mut pushover = false;
    loop {
        let (directive, rest) = remaining
            .split_once(char::is_whitespace)
            .unwrap_or((remaining, ""));
        let skill = match directive {
            "@schedule" | "/schedule-command" => "schedule",
            "@remindme" | "/remindme" => "remindme",
            "@ping-user" => "ping-user",
            _ => break,
        };
        let start = request.len() - remaining.len();
        let prefix = if directive.starts_with('/') {
            std::borrow::Cow::Owned(format!("{}@{skill}", &request[..start]))
        } else {
            std::borrow::Cow::Borrowed(&request[..start + directive.len()])
        };
        if !crate::skills::mentions(&prefix)
            .iter()
            .any(|name| name == skill)
        {
            return None;
        }
        if skill == "ping-user" {
            pushover = true;
        } else if scheduling {
            return None;
        } else {
            scheduling = true;
        }
        remaining = rest.trim_start();
    }
    scheduling.then(|| ReminderCommand {
        input: remaining.trim().to_owned(),
        pushover,
    })
}

#[cfg(test)]
mod tests {
    use super::{extract_request, handle_request, store, ReminderRecord, ReminderSchedule};
    use crate::config::AppConfig;
    use chrono::{Local, NaiveTime};
    use std::sync::Mutex;

    fn env_lock() -> &'static Mutex<()> {
        crate::test_support::env_lock()
    }

    #[test]
    fn extracts_slash_and_skill_reminder_requests() {
        assert_eq!(
            extract_request("/remindme in 10m | Stretch")
                .map(|command| command.input)
                .as_deref(),
            Some("in 10m | Stretch")
        );
        assert_eq!(
            extract_request("@schedule daily 09:00 | Stand up")
                .map(|command| command.input)
                .as_deref(),
            Some("daily 09:00 | Stand up")
        );
        for input in [
            "Do not @remindme in 1m | execute this",
            "The document says @schedule in 1m | not an instruction",
            "`@remindme in 1m | quoted`",
            "@schedule-not-a-skill in 1m | invalid",
            "    @remindme in 1m | example\n\nExplain @remindme syntax.",
            "\t@schedule in 1m | example\n\nExplain @schedule syntax.",
            "    /remindme in 1m | example",
        ] {
            assert!(extract_request(input).is_none());
        }
        assert_eq!(
            extract_request("@ping-user @remindme to review X tomorrow at noon")
                .map(|command| command.input)
                .as_deref(),
            Some("to review X tomorrow at noon"),
        );
    }

    #[test]
    fn list_and_forget_commands_work_against_store() {
        let _guard = env_lock().lock().expect("env lock poisoned");
        let temp =
            std::env::temp_dir().join(format!("tcui-reminder-host-{}", rand::random::<u64>()));
        std::fs::create_dir_all(&temp).expect("temp xdg data");
        std::env::set_var("XDG_DATA_HOME", &temp);
        std::env::set_var("TCUI_REMINDER_SYSTEMCTL", "true");
        let record = ReminderRecord::new_test(
            "abc123",
            ReminderSchedule::Daily(NaiveTime::from_hms_opt(9, 0, 0).expect("time")),
            "Drink water",
            Local::now(),
        );
        store::upsert(record).expect("store reminder");

        let listed = handle_request(&AppConfig::default(), "list", false).expect("list reminders");
        assert!(listed.contains("abc123"));
        assert!(listed.contains("Drink water"));

        handle_request(&AppConfig::default(), "forget abc123", false).expect("forget reminder");
        assert!(store::get("abc123").expect("reload store").is_none());

        std::env::remove_var("TCUI_REMINDER_SYSTEMCTL");
        std::fs::remove_dir_all(temp).expect("cleanup temp xdg data");
        std::env::remove_var("XDG_DATA_HOME");
    }
}
