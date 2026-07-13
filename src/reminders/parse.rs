use chrono::{Local, NaiveDateTime, NaiveTime, TimeZone, Weekday};

use super::{ReminderError, ReminderRequest, ReminderSchedule};

pub(super) fn parse_request(input: &str) -> Result<ReminderRequest, ReminderError> {
    let (schedule, message) = input.split_once('|').ok_or_else(|| {
        ReminderError::Invalid(
            "use `in <duration> | <message>`, `at <YYYY-MM-DD HH:MM> | <message>`, `daily <HH:MM> | <message>`, `weekly <mon|tue|...> <HH:MM> | <message>`, or `calendar <expr> | <message>`".to_string(),
        )
    })?;
    let message = message.trim();
    if message.is_empty() {
        return Err(ReminderError::Invalid(
            "reminder message cannot be empty".to_string(),
        ));
    }
    let schedule = parse_schedule(schedule.trim())?;
    Ok(ReminderRequest {
        schedule,
        message: message.to_string(),
    })
}

fn parse_schedule(input: &str) -> Result<ReminderSchedule, ReminderError> {
    if let Some(rest) = input.strip_prefix("in ") {
        return Ok(ReminderSchedule::After(parse_duration(rest.trim())?));
    }
    if let Some(rest) = input.strip_prefix("at ") {
        let datetime = parse_datetime(rest.trim())?;
        return Ok(ReminderSchedule::At(datetime));
    }
    if let Some(rest) = input.strip_prefix("daily ") {
        let time = parse_time(rest.trim())?;
        return Ok(ReminderSchedule::Daily(time));
    }
    if let Some(rest) = input.strip_prefix("weekly ") {
        let (weekday, time) = rest.split_once(char::is_whitespace).ok_or_else(|| {
            ReminderError::Invalid("weekly reminders use `weekly <day> <HH:MM>`".to_string())
        })?;
        return Ok(ReminderSchedule::Weekly(
            parse_weekday(weekday.trim())?,
            parse_time(time.trim())?,
        ));
    }
    if let Some(rest) = input.strip_prefix("calendar ") {
        let expression = rest.trim();
        if expression.is_empty() {
            return Err(ReminderError::Invalid(
                "calendar reminders need a systemd calendar expression".to_string(),
            ));
        }
        return Ok(ReminderSchedule::Calendar(expression.to_string()));
    }
    Err(ReminderError::Invalid(
        "unsupported reminder schedule".to_string(),
    ))
}

fn parse_datetime(input: &str) -> Result<chrono::DateTime<Local>, ReminderError> {
    let naive = NaiveDateTime::parse_from_str(input, "%Y-%m-%d %H:%M:%S")
        .or_else(|_| NaiveDateTime::parse_from_str(input, "%Y-%m-%d %H:%M"))
        .map_err(|_| {
            ReminderError::Invalid(
                "use `at YYYY-MM-DD HH:MM` or `at YYYY-MM-DD HH:MM:SS`".to_string(),
            )
        })?;
    Local.from_local_datetime(&naive).single().ok_or_else(|| {
        ReminderError::Invalid("ambiguous or invalid local reminder time".to_string())
    })
}

fn parse_time(input: &str) -> Result<NaiveTime, ReminderError> {
    NaiveTime::parse_from_str(input, "%H:%M:%S")
        .or_else(|_| NaiveTime::parse_from_str(input, "%H:%M"))
        .map_err(|_| ReminderError::Invalid("use `HH:MM` or `HH:MM:SS`".to_string()))
}

fn parse_weekday(input: &str) -> Result<Weekday, ReminderError> {
    match input.to_ascii_lowercase().as_str() {
        "mon" | "monday" => Ok(Weekday::Mon),
        "tue" | "tues" | "tuesday" => Ok(Weekday::Tue),
        "wed" | "wednesday" => Ok(Weekday::Wed),
        "thu" | "thurs" | "thursday" => Ok(Weekday::Thu),
        "fri" | "friday" => Ok(Weekday::Fri),
        "sat" | "saturday" => Ok(Weekday::Sat),
        "sun" | "sunday" => Ok(Weekday::Sun),
        _ => Err(ReminderError::Invalid(
            "use mon, tue, wed, thu, fri, sat, or sun".to_string(),
        )),
    }
}

fn parse_duration(input: &str) -> Result<std::time::Duration, ReminderError> {
    let mut seconds = 0_u64;
    let mut digits = String::new();
    for ch in input.chars() {
        if ch.is_ascii_digit() {
            digits.push(ch);
            continue;
        }
        if ch.is_ascii_whitespace() {
            continue;
        }
        let value = digits
            .parse::<u64>()
            .map_err(|_| ReminderError::Invalid("invalid duration number".to_string()))?;
        digits.clear();
        seconds = seconds
            .checked_add(match ch {
                's' => value,
                'm' => value * 60,
                'h' => value * 60 * 60,
                'd' => value * 60 * 60 * 24,
                _ => {
                    return Err(ReminderError::Invalid(
                        "duration units are s, m, h, and d".to_string(),
                    ));
                }
            })
            .ok_or_else(|| ReminderError::Invalid("duration is too large".to_string()))?;
    }
    if !digits.is_empty() {
        return Err(ReminderError::Invalid(
            "duration must end with a unit like 10m or 2h30m".to_string(),
        ));
    }
    if seconds == 0 {
        return Err(ReminderError::Invalid(
            "duration must be greater than zero".to_string(),
        ));
    }
    Ok(std::time::Duration::from_secs(seconds))
}

pub(super) fn weekday_label(weekday: Weekday) -> &'static str {
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

pub(super) fn describe_schedule(schedule: &ReminderSchedule) -> String {
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
    use super::{describe_schedule, parse_request};
    use crate::reminders::ReminderSchedule;
    use chrono::{Timelike, Weekday};

    #[test]
    fn parses_relative_reminder() {
        let parsed = parse_request("in 2h30m | Stretch").expect("relative reminder");
        assert_eq!(parsed.message, "Stretch");
        assert!(matches!(
            parsed.schedule,
            ReminderSchedule::After(duration) if duration.as_secs() == 9_000
        ));
    }

    #[test]
    fn parses_weekly_reminder() {
        let parsed = parse_request("weekly mon 09:30 | Plan week").expect("weekly reminder");
        assert!(matches!(
            parsed.schedule,
            ReminderSchedule::Weekly(Weekday::Mon, time)
                if time.hour() == 9 && time.minute() == 30
        ));
    }

    #[test]
    fn describe_daily_schedule_is_stable() {
        let parsed = parse_request("daily 08:15 | Stand up").expect("daily reminder");
        assert_eq!(describe_schedule(&parsed.schedule), "daily 08:15:00");
    }
}
