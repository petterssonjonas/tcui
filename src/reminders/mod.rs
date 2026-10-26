mod execution;
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
            display_schedule: describe_schedule(&request.schedule),
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
            display_schedule: describe_schedule(&schedule),
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
    #[error("reminder action cancelled")]
    Cancelled,
    #[error("Pushover reminder unavailable: {0}")]
    Notification(color_eyre::Report),
}

pub(crate) async fn dispatch(config: &AppConfig, id: &str) -> color_eyre::Result<()> {
    let guard = ping::JobGuard::wait(id).await?;
    let Some(record) = store::get(id)? else {
        return Ok(());
    };
    let result = match record.message_text() {
        Ok(message) => {
            let _ = crate::notifications::notify_reminder(config, &message).await;
            println!("tcui reminder: {message}");
            Ok(())
        }
        Err(error) => Err(color_eyre::Report::from(error)),
    };
    if !record.recurring {
        ping::finish(&guard, id, &record.unit)?;
    }
    result
}

pub(crate) async fn dispatch_ping(id: &str) -> color_eyre::Result<()> {
    ping::dispatch(id).await
}

pub(crate) fn cleanup(id: &str) -> color_eyre::Result<()> {
    ping::cleanup(id)
}

#[derive(Deserialize)]
#[serde(tag = "action", rename_all = "snake_case", deny_unknown_fields)]
enum AgentAction<'a> {
    Create {
        message: &'a str,
        #[serde(default)]
        pushover: bool,
        after_seconds: Option<u64>,
        at: Option<&'a str>,
        daily: Option<&'a str>,
        #[serde(borrow)]
        weekly: Option<WeeklyTime<'a>>,
        calendar: Option<&'a str>,
    },
    List {},
    Cancel {
        id: &'a str,
    },
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct WeeklyTime<'a> {
    day: &'a str,
    time: &'a str,
}

#[cfg(test)]
pub(crate) fn agent_tool(
    config: &AppConfig,
    arguments: &serde_json::Value,
) -> Result<String, ReminderError> {
    agent_tool_inner(config, arguments, &execution::Cancellation::default()).map(Completed::commit)
}

/// Execute an agent reminder action with request-owned cancellation.
///
/// Dropping this future stops native scheduler children. A newly registered
/// reminder remains guarded until this future observes its receipt; otherwise
/// the worker rolls back the timer, metadata, and private delivery snapshot.
pub(crate) async fn agent_tool_cancellable(
    config: AppConfig,
    arguments: serde_json::Value,
) -> Result<String, ReminderError> {
    execution::run(config, arguments).await
}

fn agent_tool_inner(
    config: &AppConfig,
    arguments: &serde_json::Value,
    cancellation: &execution::Cancellation,
) -> Result<Completed, ReminderError> {
    cancellation.check()?;
    let action = AgentAction::deserialize(arguments)
        .map_err(|error| ReminderError::Invalid(format!("Invalid reminder arguments: {error}")))?;
    match action {
        AgentAction::List {} => list_reminders().map(Completed::plain),
        AgentAction::Cancel { id } => cancel_reminder(id, cancellation).map(Completed::plain),
        AgentAction::Create {
            message,
            pushover,
            after_seconds,
            at,
            daily,
            weekly,
            calendar,
        } => {
            if ["after_seconds", "at", "daily", "weekly", "calendar"]
                .iter()
                .filter(|field| arguments.get(**field).is_some())
                .count()
                != 1
            {
                return Err(ReminderError::Invalid(
                    "create requires exactly one timing field: after_seconds, at, daily, weekly, or calendar"
                        .to_owned(),
                ));
            }
            if message.trim().is_empty() || message.contains('\0') {
                return Err(ReminderError::Invalid(
                    "reminder message must be nonempty and cannot contain NUL".to_owned(),
                ));
            }
            let now = Local::now();
            let schedule = if let Some(seconds) = after_seconds {
                let duration = std::time::Duration::from_secs(seconds);
                if seconds == 0
                    || chrono::Duration::from_std(duration)
                        .ok()
                        .and_then(|duration| now.checked_add_signed(duration))
                        .is_none()
                {
                    return Err(ReminderError::Invalid(
                        "after_seconds must be positive and within the supported date range"
                            .to_owned(),
                    ));
                }
                ReminderSchedule::After(duration)
            } else if let Some(at) = at {
                let datetime = DateTime::parse_from_rfc3339(at)
                    .map_err(|_| ReminderError::Invalid("at must be an RFC3339 timestamp".into()))?
                    .with_timezone(&Local);
                // systemd registers these deadlines at whole-second precision.
                use chrono::Timelike;
                if datetime.timestamp() <= now.timestamp() || datetime.nanosecond() >= 1_000_000_000
                {
                    return Err(ReminderError::Invalid(
                        "the reminder time must be in the future".to_owned(),
                    ));
                }
                systemd::validate_calendar(&systemd::absolute_calendar(&datetime), cancellation)?;
                ReminderSchedule::At(datetime)
            } else if let Some(time) = daily {
                ReminderSchedule::Daily(reminder_time(time)?)
            } else if let Some(weekly) = weekly {
                let day = match weekly.day {
                    "mon" => Weekday::Mon,
                    "tue" => Weekday::Tue,
                    "wed" => Weekday::Wed,
                    "thu" => Weekday::Thu,
                    "fri" => Weekday::Fri,
                    "sat" => Weekday::Sat,
                    "sun" => Weekday::Sun,
                    _ => {
                        return Err(ReminderError::Invalid(
                            "weekly day must be mon, tue, wed, thu, fri, sat, or sun".into(),
                        ));
                    }
                };
                ReminderSchedule::Weekly(day, reminder_time(weekly.time)?)
            } else if let Some(expression) = calendar {
                systemd::validate_calendar(expression, cancellation)?;
                ReminderSchedule::Calendar(expression.to_owned())
            } else {
                return Err(ReminderError::Invalid(
                    "the timing field cannot be null".to_owned(),
                ));
            };
            if pushover && !config.notifications.pushover.enabled {
                return Err(ReminderError::Invalid(
                    "Pushover is disabled. Configure and enable it before scheduling a Pushover reminder."
                        .to_owned(),
                ));
            }
            if config.notifications.pushover.enabled && message.chars().count() > 1024 {
                return Err(ReminderError::Invalid(
                    "Pushover reminder messages must contain 1–1024 characters".to_owned(),
                ));
            }
            schedule_request(
                config,
                ReminderRequest {
                    schedule,
                    message: message.to_owned(),
                },
                cancellation,
            )
        }
    }
}

pub(crate) fn agent_tool_schema() -> serde_json::Value {
    serde_json::json!({
        "oneOf": [
            {
                "type": "object",
                "additionalProperties": false,
                "required": ["action", "message"],
                "description": "Create one reminder with exactly one timing field. Daily and weekly times use the scheduler host's timezone.",
                "properties": {
                    "action": {"const": "create"},
                    "message": {"type": "string", "minLength": 1},
                    "pushover": {
                        "type": "boolean",
                        "default": false,
                        "description": "Require enabled Pushover and available credentials. False still uses Pushover when configured."
                    },
                    "after_seconds": {"type": "integer", "minimum": 1},
                    "at": {
                        "type": "string",
                        "format": "date-time",
                        "description": "Future RFC3339 timestamp with an explicit UTC offset."
                    },
                    "daily": {
                        "type": "string",
                        "pattern": "^([01][0-9]|2[0-3]):[0-5][0-9](:[0-5][0-9])?$"
                    },
                    "weekly": {
                        "type": "object",
                        "additionalProperties": false,
                        "required": ["day", "time"],
                        "properties": {
                            "day": {"enum": ["mon", "tue", "wed", "thu", "fri", "sat", "sun"]},
                            "time": {"type": "string", "pattern": "^([01][0-9]|2[0-3]):[0-5][0-9](:[0-5][0-9])?$"}
                        }
                    },
                    "calendar": {
                        "type": "string",
                        "minLength": 1,
                        "description": "Systemd calendar expression with a future occurrence, in the scheduler host's timezone unless explicitly specified."
                    }
                },
                "oneOf": [
                    {"required": ["after_seconds"]},
                    {"required": ["at"]},
                    {"required": ["daily"]},
                    {"required": ["weekly"]},
                    {"required": ["calendar"]}
                ]
            },
            {
                "type": "object",
                "additionalProperties": false,
                "required": ["action"],
                "properties": {"action": {"const": "list"}}
            },
            {
                "type": "object",
                "additionalProperties": false,
                "required": ["action", "id"],
                "properties": {
                    "action": {"const": "cancel"},
                    "id": {"type": "string", "pattern": "^[a-zA-Z0-9]+$"}
                }
            }
        ]
    })
}

fn reminder_time(input: &str) -> Result<NaiveTime, ReminderError> {
    if !matches!(input.len(), 5 | 8)
        || !input.bytes().enumerate().all(|(index, byte)| {
            if index == 2 || index == 5 {
                byte == b':'
            } else {
                byte.is_ascii_digit()
            }
        })
    {
        return Err(ReminderError::Invalid("use HH:MM or HH:MM:SS".into()));
    }
    NaiveTime::parse_from_str(input, "%H:%M:%S")
        .or_else(|_| NaiveTime::parse_from_str(input, "%H:%M"))
        .ok()
        .filter(|time| {
            use chrono::Timelike;
            time.nanosecond() == 0
        })
        .ok_or_else(|| ReminderError::Invalid("invalid HH:MM or HH:MM:SS time".into()))
}

struct Completed {
    receipt: String,
    registration: Option<Registration>,
}

impl Completed {
    fn plain(receipt: String) -> Self {
        Self {
            receipt,
            registration: None,
        }
    }

    #[cfg(test)]
    fn commit(self) -> String {
        if let Some(registration) = self.registration {
            registration.commit();
        }
        self.receipt
    }
}

struct Registration {
    record: ReminderRecord,
    guard: ping::JobGuard,
    timer_started: bool,
    pending: bool,
}

impl Registration {
    fn commit(mut self) {
        self.pending = false;
    }

    fn rollback(&mut self) -> Result<(), ReminderError> {
        self.pending = false;
        let timer = if self.timer_started {
            // Recovery must run even after request cancellation. This is the
            // only scheduler work allowed to outlive the requesting future.
            systemd::cancel(&self.record.unit)
        } else {
            Ok(())
        };
        let payload = ping::remove(&self.guard).map_err(ReminderError::Notification);
        // Retain metadata when either cleanup fails so list/cancel can recover
        // the job. Remove private credentials even if the timer cannot stop.
        if timer.is_ok() && payload.is_ok() {
            store::remove(&self.record.id)?;
        }
        timer?;
        payload
    }

    fn failed(mut self, error: ReminderError) -> ReminderError {
        match self.rollback() {
            Ok(()) => error,
            Err(cleanup) => ReminderError::Schedule(format!(
                "{error}; reminder cleanup failed: {cleanup}. Cancel reminder {} before retrying",
                self.record.id,
            )),
        }
    }
}

impl Drop for Registration {
    fn drop(&mut self) {
        if self.pending && self.rollback().is_err() {
            // Do not log scheduler-controlled diagnostics or private payloads.
            eprintln!(
                "Could not clean up unobserved reminder {}; list and cancel this reminder before retrying",
                self.record.id,
            );
        }
    }
}

fn schedule_request(
    config: &AppConfig,
    request: ReminderRequest,
    cancellation: &execution::Cancellation,
) -> Result<Completed, ReminderError> {
    cancellation.check()?;
    let delivery =
        crate::notifications::pushover::prepare(config).map_err(ReminderError::Notification)?;
    cancellation.check()?;
    let mut record = ReminderRecord::new(&request, String::new())?;
    record.unit = format!("tcui-reminder-{}", record.id);
    record.pushover = delivery.is_some();
    let guard = ping::JobGuard::open(&record.id)
        .map_err(|error| ReminderError::Schedule(format!("Cannot lock reminder state: {error}")))?;
    let mut registration = Registration {
        record,
        guard,
        timer_started: false,
        pending: true,
    };
    let operation = (|| {
        cancellation.check()?;
        store::upsert_cancellable(registration.record.clone(), cancellation)?;
        cancellation.check()?;
        if let Some(delivery) = delivery {
            ping::save(
                &registration.guard,
                delivery,
                &request.message,
                registration.record.recurring,
                cancellation,
            )
            .map_err(ReminderError::Notification)?;
        }
        cancellation.check()?;
        registration.timer_started = true;
        systemd::schedule(
            &registration.record.id,
            &request.schedule,
            registration.record.pushover,
            cancellation,
        )?;
        cancellation.check()
    })();
    if let Err(error) = operation {
        return Err(registration.failed(error));
    }
    let record = &registration.record;
    let receipt = format!(
        "Scheduled {} reminder {} (ID {}): {}{}",
        if request.schedule.is_recurring() {
            "recurring"
        } else {
            "one-shot"
        },
        describe_schedule(&request.schedule),
        record.id,
        request.message,
        if record.pushover {
            "\nPushover jobs keep plaintext messages and credentials in private files. One-shot files are removed after the timer attempts delivery; recurring files remain until cancellation."
        } else {
            ""
        },
    );
    Ok(Completed {
        receipt,
        registration: Some(registration),
    })
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

fn cancel_reminder(
    id: &str,
    cancellation: &execution::Cancellation,
) -> Result<String, ReminderError> {
    if id.is_empty() || !id.bytes().all(|byte| byte.is_ascii_alphanumeric()) {
        return Err(ReminderError::Invalid(
            "id must contain only ASCII letters and digits".to_owned(),
        ));
    }
    let guard = ping::JobGuard::open(id)
        .map_err(|error| ReminderError::Schedule(format!("Cannot lock reminder state: {error}")))?;
    cancellation.check()?;
    let Some(record) = store::get(id)? else {
        return Err(ReminderError::Invalid(format!(
            "reminder `{id}` was not found"
        )));
    };
    systemd::cancel_cancellable(&record.unit, cancellation)?;
    cancellation.check()?;
    if record.pushover {
        ping::remove(&guard).map_err(ReminderError::Notification)?;
    }
    store::remove(id)?;
    Ok(format!("Cancelled reminder {id}."))
}

fn weekday_label(weekday: Weekday) -> &'static str {
    match weekday {
        Weekday::Mon => "Mon",
        Weekday::Tue => "Tue",
        Weekday::Wed => "Wed",
        Weekday::Thu => "Thu",
        Weekday::Fri => "Fri",
        Weekday::Sat => "Sat",
        Weekday::Sun => "Sun",
    }
}

fn describe_schedule(schedule: &ReminderSchedule) -> String {
    match schedule {
        ReminderSchedule::After(duration) => {
            format!("in {}", format_duration(*duration))
        }
        ReminderSchedule::At(datetime) => datetime.format("at %Y-%m-%d %H:%M:%S").to_string(),
        ReminderSchedule::Daily(time) => time.format("daily %H:%M:%S").to_string(),
        ReminderSchedule::Weekly(weekday, time) => {
            format!(
                "weekly {} {}",
                weekday_label(*weekday),
                time.format("%H:%M:%S")
            )
        }
        ReminderSchedule::Calendar(expression) => format!("calendar {expression}"),
    }
}

fn format_duration(duration: std::time::Duration) -> String {
    let mut seconds = duration.as_secs();
    let days = seconds / 86_400;
    seconds %= 86_400;
    let hours = seconds / 3_600;
    seconds %= 3_600;
    let minutes = seconds / 60;
    seconds %= 60;
    let mut parts = Vec::new();
    if days > 0 {
        parts.push(format!("{days}d"));
    }
    if hours > 0 {
        parts.push(format!("{hours}h"));
    }
    if minutes > 0 {
        parts.push(format!("{minutes}m"));
    }
    if seconds > 0 || parts.is_empty() {
        parts.push(format!("{seconds}s"));
    }
    parts.join("")
}

#[cfg(test)]
mod tests {
    use super::{agent_tool, store, ReminderRecord, ReminderSchedule};
    use crate::config::AppConfig;
    use chrono::{Local, NaiveTime};
    use std::sync::Mutex;

    fn env_lock() -> &'static Mutex<()> {
        crate::test_support::env_lock()
    }

    #[test]
    fn list_and_cancel_actions_work_against_store() {
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

        let listed = agent_tool(
            &AppConfig::default(),
            &serde_json::json!({"action": "list"}),
        )
        .expect("list reminders");
        assert!(listed.contains("abc123"));
        assert!(listed.contains("Drink water"));

        agent_tool(
            &AppConfig::default(),
            &serde_json::json!({"action": "cancel", "id": "abc123"}),
        )
        .expect("cancel reminder");
        assert!(store::get("abc123").expect("reload store").is_none());

        std::env::remove_var("TCUI_REMINDER_SYSTEMCTL");
        std::fs::remove_dir_all(temp).expect("cleanup temp xdg data");
        std::env::remove_var("XDG_DATA_HOME");
    }
}
