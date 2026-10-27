use super::execution::{self, Cancellation};
use super::{weekday_label, ReminderError, ReminderSchedule};

pub(super) fn absolute_calendar(datetime: &chrono::DateTime<chrono::Local>) -> String {
    datetime
        .with_timezone(&chrono::Utc)
        .format("%Y-%m-%d %H:%M:%S UTC")
        .to_string()
}

pub(super) fn validate_calendar(
    expression: &str,
    cancellation: &Cancellation,
) -> Result<(), ReminderError> {
    if expression.trim().is_empty() || expression.chars().any(char::is_control) {
        return Err(ReminderError::Invalid(
            "calendar must be a nonempty systemd calendar expression without control characters"
                .to_owned(),
        ));
    }
    let analyzer = SchedulerContext::new("systemd-analyze", "TCUI_REMINDER_SYSTEMD_ANALYZE")?;
    let output = execution::output(
        analyzer
            .command()
            .args(["calendar", "--iterations=1", "--", expression]),
        cancellation,
    )?;
    if !output.status.success() {
        return Err(ReminderError::Invalid(
            "calendar is not a valid systemd calendar expression".to_owned(),
        ));
    }
    let text = String::from_utf8_lossy(&output.stdout);
    let next = text
        .lines()
        .find_map(|line| line.trim().strip_prefix("Next elapse:"))
        .map(str::trim);
    if next.is_none_or(|value| value.is_empty() || value == "never" || value == "n/a") {
        return Err(ReminderError::Invalid(
            "calendar must have a future occurrence".to_owned(),
        ));
    }
    Ok(())
}

pub(super) fn schedule(
    id: &str,
    schedule: &ReminderSchedule,
    pushover: bool,
    cancellation: &Cancellation,
) -> Result<String, ReminderError> {
    let unit = format!("tcui-reminder-{id}");
    let executable = std::env::current_exe()?;
    let scheduler = SchedulerContext::new("systemd-run", "TCUI_REMINDER_SYSTEMD_RUN")?;
    let mut command = scheduler.command();
    command
        .arg("--user")
        .arg("--collect")
        .arg("--timer-property=AccuracySec=1s")
        .arg("--property=UMask=0077")
        .arg("--unit")
        .arg(&unit);
    if !schedule.is_recurring() {
        command
            .arg("--timer-property=RemainAfterElapse=no")
            .arg("--timer-property=CollectMode=inactive-or-failed")
            .arg(cleanup_command(
                &executable,
                scheduler.toolbox.as_deref(),
                id,
            )?);
    }
    set_notification_environment(&mut command, |key| std::env::var_os(key));
    match schedule {
        ReminderSchedule::After(duration) => {
            command
                .arg("--on-active")
                .arg(format!("{}s", duration.as_secs()));
        }
        ReminderSchedule::At(datetime) => {
            command
                .arg("--on-calendar")
                .arg(absolute_calendar(datetime));
        }
        ReminderSchedule::Daily(time) => {
            command
                .arg("--on-calendar")
                .arg(format!("*-*-* {}", time.format("%H:%M:%S")));
        }
        ReminderSchedule::Weekly(weekday, time) => {
            command.arg("--on-calendar").arg(format!(
                "{} *-*-* {}",
                weekday_label(*weekday),
                time.format("%H:%M:%S")
            ));
        }
        ReminderSchedule::Calendar(expression) => {
            command.arg("--on-calendar").arg(expression);
        }
    }
    command.arg("--");
    if let Some(container) = &scheduler.toolbox {
        command.args(["toolbox", "run", "--container", container, "--"]);
    }
    command
        .arg(executable)
        .arg(if pushover {
            "reminder-ping-dispatch"
        } else {
            "reminder-dispatch"
        })
        .arg(id);
    let output = execution::output(&mut command, cancellation)?;
    if output.status.success() {
        Ok(unit)
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        let message = if stderr.is_empty() {
            "systemd-run failed to register the reminder".to_string()
        } else {
            stderr
        };
        Err(ReminderError::Schedule(message))
    }
}

fn cleanup_command(
    executable: &std::path::Path,
    toolbox: Option<&str>,
    id: &str,
) -> Result<String, ReminderError> {
    let mut command = String::from("--property=ExecStopPost=:");
    if let Some(container) = toolbox {
        for argument in ["toolbox", "run", "--container", container, "--"] {
            push_unit_argument(&mut command, argument)?;
        }
    }
    push_unit_argument(
        &mut command,
        executable.to_str().ok_or_else(|| {
            ReminderError::Schedule("TCUI's executable path must be UTF-8 for timer cleanup".into())
        })?,
    )?;
    for argument in ["reminder-cleanup", id] {
        push_unit_argument(&mut command, argument)?;
    }
    Ok(command)
}

fn push_unit_argument(command: &mut String, argument: &str) -> Result<(), ReminderError> {
    if argument.chars().any(char::is_control) {
        return Err(ReminderError::Schedule(
            "Invalid timer cleanup argument".into(),
        ));
    }
    if !command.ends_with(':') {
        command.push(' ');
    }
    command.push('"');
    for character in argument.chars() {
        match character {
            '\\' | '"' => command.push('\\'),
            '%' => command.push('%'),
            _ => {}
        }
        command.push(character);
    }
    command.push('"');
    Ok(())
}

fn set_notification_environment(
    command: &mut std::process::Command,
    get: impl Fn(&str) -> Option<std::ffi::OsString>,
) {
    // Timers run outside the originating terminal. Preserve only routing context:
    // otherwise an SSH reminder could notify the server's unrelated desktop.
    // Empty overrides also prevent stale user-manager context leaking into timers.
    for key in [
        "HOME",
        "PATH",
        "XDG_CONFIG_HOME",
        "XDG_DATA_HOME",
        "XDG_CACHE_HOME",
        "XDG_STATE_HOME",
        "XDG_RUNTIME_DIR",
        "TZ",
        "SSH_CONNECTION",
        "SSH_CLIENT",
        "SSH_TTY",
        "HERDR_ENV",
        "HERDR_PANE_ID",
        "HERDR_BIN_PATH",
        "HERDR_SOCKET_PATH",
    ] {
        let mut argument = std::ffi::OsString::from(format!("--setenv={key}="));
        argument.push(get(key).unwrap_or_default());
        command.arg(argument);
    }
}

pub(super) fn cancel(unit: &str) -> Result<(), ReminderError> {
    cancel_cancellable(unit, &Cancellation::cleanup())
}

pub(super) fn cancel_cancellable(
    unit: &str,
    cancellation: &Cancellation,
) -> Result<(), ReminderError> {
    stop_units(unit, &["timer", "service"], true, cancellation)
}

pub(super) fn complete(unit: &str) -> Result<(), ReminderError> {
    // Stop ordering reverses timer Before=service. ExecStopPost must not wait
    // for its own service to finish stopping before the timer can stop.
    stop_units(unit, &["timer"], false, &Cancellation::default())
}

fn stop_units(
    unit: &str,
    kinds: &[&str],
    wait: bool,
    cancellation: &Cancellation,
) -> Result<(), ReminderError> {
    let suffix = unit.strip_prefix("tcui-reminder-").filter(|suffix| {
        !suffix.is_empty() && suffix.bytes().all(|byte| byte.is_ascii_alphanumeric())
    });
    if suffix.is_none() {
        return Err(ReminderError::Invalid("invalid reminder unit".to_owned()));
    }
    let controller = SchedulerContext::new("systemctl", "TCUI_REMINDER_SYSTEMCTL")?;
    let mut failure = None;
    for kind in kinds {
        let name = format!("{unit}.{kind}");
        let mut command = controller.command();
        command.arg("--user");
        if !wait {
            command.arg("--no-block");
        }
        let output = execution::output(command.args(["stop", "--", &name]), cancellation)?;
        if output.status.success() {
            continue;
        }
        let state = execution::output(
            controller.command().args([
                "--user",
                "show",
                "--property=LoadState",
                "--value",
                "--",
                &name,
            ]),
            cancellation,
        )?;
        if state.status.success() && state.stdout.trim_ascii() == b"not-found" {
            continue;
        }
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_owned();
        failure.get_or_insert_with(|| {
            ReminderError::Schedule(if stderr.is_empty() {
                format!("failed to stop reminder unit {name}")
            } else {
                stderr
            })
        });
    }
    match failure {
        Some(error) => Err(error),
        None => Ok(()),
    }
}

struct SchedulerContext {
    program: std::ffi::OsString,
    toolbox: Option<String>,
}

impl SchedulerContext {
    fn new(program: &str, override_name: &str) -> Result<Self, ReminderError> {
        if let Some(program) = std::env::var_os(override_name) {
            return Ok(Self {
                program,
                toolbox: None,
            });
        }
        let toolbox = if std::path::Path::new("/run/.toolboxenv").exists() {
            use std::io::Read;
            let mut content = String::new();
            std::fs::File::open("/run/.containerenv")?
                .take(4097)
                .read_to_string(&mut content)?;
            if content.len() > 4096 {
                return Err(ReminderError::Schedule(
                    "Toolbx metadata is too large".to_owned(),
                ));
            }
            let value = content
                .lines()
                .find_map(|line| line.strip_prefix("name="))
                .ok_or_else(|| {
                    ReminderError::Schedule("Cannot identify the originating Toolbx".to_owned())
                })?;
            let name: String = serde_json::from_str(value)
                .map_err(|_| ReminderError::Schedule("Invalid Toolbx container name".to_owned()))?;
            if name.is_empty()
                || name.len() > 255
                || name.starts_with('-')
                || !name
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'.'))
            {
                return Err(ReminderError::Schedule(
                    "Invalid Toolbx container name".to_owned(),
                ));
            }
            Some(name)
        } else {
            None
        };
        Ok(Self {
            program: program.into(),
            toolbox,
        })
    }

    fn command(&self) -> std::process::Command {
        let mut command = if self.toolbox.is_some() {
            let mut command = std::process::Command::new("flatpak-spawn");
            command
                .arg("--host")
                .arg("--watch-bus")
                .arg("--env=LC_ALL=C")
                .arg(&self.program);
            command
        } else {
            std::process::Command::new(&self.program)
        };
        command.env("LC_ALL", "C");
        command
    }
}

#[cfg(test)]
mod tests {
    use super::cancel;
    use std::sync::Mutex;

    fn env_lock() -> &'static Mutex<()> {
        crate::test_support::env_lock()
    }

    #[test]
    fn missing_unit_is_treated_as_already_gone() {
        let _guard = env_lock().lock().expect("env lock poisoned");
        let temp = std::env::temp_dir().join(format!("tcui-systemctl-{}", rand::random::<u64>()));
        let script = temp.join("systemctl-mock.sh");
        std::fs::create_dir_all(&temp).expect("temp dir");
        std::fs::write(
            &script,
            "#!/bin/sh\nif [ \"$2\" = show ]; then printf 'not-found\\n'; exit 0; fi\nprintf 'Unit %s not loaded.\\n' \"$4\" >&2\nexit 1\n",
        )
        .expect("write script");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut perms = std::fs::metadata(&script).expect("metadata").permissions();
            perms.set_mode(0o755);
            std::fs::set_permissions(&script, perms).expect("set perms");
        }

        std::env::set_var("TCUI_REMINDER_SYSTEMCTL", &script);
        cancel("tcui-reminder-test").expect("ignore missing reminder");
        std::env::remove_var("TCUI_REMINDER_SYSTEMCTL");
        std::fs::remove_dir_all(temp).expect("cleanup temp dir");
    }
}
