mod parse;
mod store;
mod systemd;

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
}

impl ReminderRecord {
    fn new(request: &ReminderRequest, unit: String) -> Result<Self, ReminderError> {
        let id = format!("{:016x}", rand::random::<u64>());
        Ok(Self {
            id,
            display_schedule: parse::describe_schedule(&request.schedule),
            recurring: request.schedule.is_recurring(),
            message: crate::storage::Storage::encrypt_shared_text(&request.message)?,
            unit,
            created_at: Local::now().to_rfc3339(),
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
            message: crate::storage::Storage::encrypt_shared_text(message).expect("encrypt reminder"),
            unit: format!("tcui-reminder-{id}"),
            created_at: created_at.to_rfc3339(),
        }
    }

    fn message_text(&self) -> Result<String, ReminderError> {
        Ok(crate::storage::Storage::decrypt_shared_text(&self.message)?)
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
}

pub(crate) fn maybe_handle_request(config: &AppConfig, request: &str) -> Option<String> {
    let Some(command) = extract_request(request) else {
        return None;
    };
    Some(match handle_request(config, &command) {
        Ok(response) => response,
        Err(error) => format!("Reminder scheduling failed: {error}"),
    })
}

pub(crate) async fn dispatch(config: &AppConfig, id: &str) -> color_eyre::Result<()> {
    let Some(record) = store::get(id)? else {
        return Ok(());
    };
    let message = record.message_text()?;
    crate::notifications::notify_reminder(config, &message).await?;
    println!("tcui reminder: {message}");
    if !record.recurring {
        store::remove(id)?;
    }
    Ok(())
}

fn handle_request(_config: &AppConfig, input: &str) -> Result<String, ReminderError> {
    if input.eq_ignore_ascii_case("list") {
        return list_reminders();
    }
    if let Some(id) = input.strip_prefix("forget ") {
        return forget_reminder(id.trim());
    }
    if let Some(id) = input.strip_prefix("cancel ") {
        return forget_reminder(id.trim());
    }
    schedule_request(input)
}

fn schedule_request(input: &str) -> Result<String, ReminderError> {
    let request = parse::parse_request(input)?;
    let mut record = ReminderRecord::new(&request, String::new())?;
    let unit = systemd::schedule(&record.id, &request.schedule)?;
    record.unit = unit.clone();
    store::upsert(record)?;
    Ok(format!(
        "Scheduled {} reminder {}: {}",
        if request.schedule.is_recurring() {
            "recurring"
        } else {
            "one-shot"
        },
        parse::describe_schedule(&request.schedule),
        request.message
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
        let cadence = if reminder.recurring { "recurring" } else { "one-shot" };
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
    let Some(record) = store::get(id)? else {
        return Err(ReminderError::Invalid(format!("reminder `{id}` was not found")));
    };
    systemd::cancel(&record.unit)?;
    store::remove(id)?;
    Ok(format!("Forgot reminder {id}."))
}

fn extract_request(request: &str) -> Option<String> {
    let trimmed = request.trim();
    for prefix in ["/remindme", "/schedule-command"] {
        if let Some(rest) = trimmed.strip_prefix(prefix) {
            let command = rest.trim();
            return Some(command.to_string());
        }
    }
    for marker in ["@remindme", "@schedule"] {
        if let Some((_, rest)) = trimmed.rsplit_once(marker) {
            let command = rest.trim();
            return Some(command.to_string());
        }
    }
    None
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
            extract_request("/remindme in 10m | Stretch").as_deref(),
            Some("in 10m | Stretch")
        );
        assert_eq!(
            extract_request("@schedule daily 09:00 | Stand up").as_deref(),
            Some("daily 09:00 | Stand up")
        );
    }

    #[test]
    fn list_and_forget_commands_work_against_store() {
        let _guard = env_lock().lock().expect("env lock poisoned");
        let temp = std::env::temp_dir().join(format!("tcui-reminder-host-{}", rand::random::<u64>()));
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

        let listed = handle_request(&AppConfig::default(), "list").expect("list reminders");
        assert!(listed.contains("abc123"));
        assert!(listed.contains("Drink water"));

        let forgotten = handle_request(&AppConfig::default(), "forget abc123").expect("forget reminder");
        assert_eq!(forgotten, "Forgot reminder abc123.");
        assert_eq!(handle_request(&AppConfig::default(), "list").expect("list reminders"), "No reminders scheduled.");

        std::env::remove_var("TCUI_REMINDER_SYSTEMCTL");
        std::fs::remove_dir_all(temp).expect("cleanup temp xdg data");
        std::env::remove_var("XDG_DATA_HOME");
    }
}
