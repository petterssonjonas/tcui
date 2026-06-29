use super::{parse::weekday_label, ReminderError, ReminderSchedule};

pub(super) fn schedule(id: &str, schedule: &ReminderSchedule) -> Result<String, ReminderError> {
    let unit = format!("tcui-reminder-{id}");
    let executable = std::env::current_exe()?;
    let mut command = std::process::Command::new(systemd_run_program());
    command.arg("--user").arg("--collect").arg("--unit").arg(&unit);
    match schedule {
        ReminderSchedule::After(duration) => {
            command
                .arg("--on-active")
                .arg(format!("{}s", duration.as_secs()));
        }
        ReminderSchedule::At(datetime) => {
            command
                .arg("--on-calendar")
                .arg(datetime.format("%Y-%m-%d %H:%M:%S").to_string());
        }
        ReminderSchedule::Daily(time) => {
            command
                .arg("--on-calendar")
                .arg(format!("*-*-* {}", time.format("%H:%M:%S")));
        }
        ReminderSchedule::Weekly(weekday, time) => {
            command
                .arg("--on-calendar")
                .arg(format!("{} *-*-* {}", weekday_label(*weekday), time.format("%H:%M:%S")));
        }
        ReminderSchedule::Calendar(expression) => {
            command.arg("--on-calendar").arg(expression);
        }
    }
    let output = command
        .arg(executable)
        .arg("reminder-dispatch")
        .arg(id)
        .output()?;
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

fn systemd_run_program() -> std::ffi::OsString {
    std::env::var_os("TCUI_REMINDER_SYSTEMD_RUN")
        .unwrap_or_else(|| std::ffi::OsString::from("systemd-run"))
}

pub(super) fn cancel(unit: &str) -> Result<(), ReminderError> {
    let output = std::process::Command::new(systemctl_program())
        .arg("--user")
        .arg("cancel")
        .arg(unit)
        .output()?;
    if output.status.success() {
        Ok(())
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        if stderr.contains("not loaded")
            || stderr.contains("No files")
            || stderr.contains("not found")
        {
            return Ok(());
        }
        let message = if stderr.is_empty() {
            format!("failed to cancel reminder unit {unit}")
        } else {
            stderr
        };
        Err(ReminderError::Schedule(message))
    }
}

fn systemctl_program() -> std::ffi::OsString {
    std::env::var_os("TCUI_REMINDER_SYSTEMCTL")
        .unwrap_or_else(|| std::ffi::OsString::from("systemctl"))
}

#[cfg(test)]
mod tests {
    use super::{cancel, schedule};
    use crate::reminders::ReminderSchedule;
    use chrono::{Local, NaiveTime};
    use std::sync::Mutex;

    fn env_lock() -> &'static Mutex<()> {
        crate::test_support::env_lock()
    }

    fn write_success_script(name: &str) -> std::path::PathBuf {
        let temp = std::env::temp_dir().join(format!(
            "tcui-reminder-script-{}-{}",
            name,
            rand::random::<u64>()
        ));
        let script = temp.join(format!("{name}.sh"));
        std::fs::create_dir_all(&temp).expect("temp dir");
        std::fs::write(&script, "#!/bin/sh\nexit 0\n").expect("write script");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut perms = std::fs::metadata(&script).expect("metadata").permissions();
            perms.set_mode(0o755);
            std::fs::set_permissions(&script, perms).expect("set perms");
        }
        script
    }

    #[test]
    fn scheduler_command_can_be_overridden_for_tests() {
        let _guard = env_lock().lock().expect("env lock poisoned");
        let script = write_success_script("systemd-run");
        std::env::set_var("TCUI_REMINDER_SYSTEMD_RUN", &script);
        let unit = schedule(
            "test",
            &ReminderSchedule::Daily(NaiveTime::from_hms_opt(7, 30, 0).expect("time")),
        )
        .expect("schedule reminder");
        std::env::remove_var("TCUI_REMINDER_SYSTEMD_RUN");
        std::fs::remove_file(&script).expect("remove script");
        std::fs::remove_dir(script.parent().expect("script parent")).expect("remove temp dir");
        assert_eq!(unit, "tcui-reminder-test");
    }

    #[test]
    fn absolute_reminder_command_builds() {
        let _guard = env_lock().lock().expect("env lock poisoned");
        let script = write_success_script("systemd-run");
        std::env::set_var("TCUI_REMINDER_SYSTEMD_RUN", &script);
        let unit = schedule("abs", &ReminderSchedule::At(Local::now())).expect("schedule reminder");
        std::env::remove_var("TCUI_REMINDER_SYSTEMD_RUN");
        std::fs::remove_file(&script).expect("remove script");
        std::fs::remove_dir(script.parent().expect("script parent")).expect("remove temp dir");
        assert_eq!(unit, "tcui-reminder-abs");
    }

    #[test]
    fn cancel_command_can_be_overridden_for_tests() {
        let _guard = env_lock().lock().expect("env lock poisoned");
        let script = write_success_script("systemctl");
        std::env::set_var("TCUI_REMINDER_SYSTEMCTL", &script);
        cancel("tcui-reminder-test").expect("cancel reminder");
        std::env::remove_var("TCUI_REMINDER_SYSTEMCTL");
        std::fs::remove_file(&script).expect("remove script");
        std::fs::remove_dir(script.parent().expect("script parent")).expect("remove temp dir");
    }

    #[test]
    fn missing_unit_is_treated_as_already_gone() {
        let _guard = env_lock().lock().expect("env lock poisoned");
        let temp = std::env::temp_dir().join(format!("tcui-systemctl-{}", rand::random::<u64>()));
        let script = temp.join("systemctl-mock.sh");
        std::fs::create_dir_all(&temp).expect("temp dir");
        std::fs::write(
            &script,
            "#!/bin/sh\necho 'Unit tcui-reminder-test.service not loaded.' 1>&2\nexit 1\n",
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
