use serde::{Deserialize, Serialize};

use super::{ReminderError, ReminderRecord};

#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(default)]
struct ReminderStoreFile {
    reminders: Vec<ReminderRecord>,
}

pub(super) fn upsert(record: ReminderRecord) -> Result<(), ReminderError> {
    let path = store_path();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let mut file = load()?;
    file.reminders.retain(|existing| existing.id != record.id);
    file.reminders.push(record);
    save(&path, &file)
}

pub(super) fn get(id: &str) -> Result<Option<ReminderRecord>, ReminderError> {
    Ok(load()?.reminders.into_iter().find(|record| record.id == id))
}

pub(super) fn remove(id: &str) -> Result<(), ReminderError> {
    let path = store_path();
    let mut file = load()?;
    file.reminders.retain(|record| record.id != id);
    save(&path, &file)
}

pub(super) fn list() -> Result<Vec<ReminderRecord>, ReminderError> {
    let mut reminders = load()?.reminders;
    reminders.sort_by(|left, right| left.created_at.cmp(&right.created_at));
    Ok(reminders)
}

fn load() -> Result<ReminderStoreFile, ReminderError> {
    let path = store_path();
    if !path.exists() {
        return Ok(ReminderStoreFile::default());
    }
    let content = std::fs::read_to_string(path)?;
    Ok(toml::from_str(&content)?)
}

fn save(path: &std::path::Path, file: &ReminderStoreFile) -> Result<(), ReminderError> {
    std::fs::write(path, toml::to_string_pretty(file)?)?;
    Ok(())
}

fn store_path() -> std::path::PathBuf {
    dirs::data_dir()
        .unwrap_or_else(|| std::path::PathBuf::from("."))
        .join("tcui")
        .join("reminders.toml")
}

#[cfg(test)]
mod tests {
    use super::{get, list, remove, upsert};
    use crate::reminders::ReminderRecord;
    use chrono::{Local, NaiveTime};
    use std::sync::Mutex;

    fn env_lock() -> &'static Mutex<()> {
        crate::test_support::env_lock()
    }

    #[test]
    fn reminder_store_round_trip() {
        let _guard = env_lock().lock().expect("env lock poisoned");
        let temp = std::env::temp_dir().join(format!("tcui-reminders-{}", rand::random::<u64>()));
        std::fs::create_dir_all(&temp).expect("temp xdg data");
        std::env::set_var("XDG_DATA_HOME", &temp);
        let record = ReminderRecord::new_test(
            "r1",
            crate::reminders::ReminderSchedule::Daily(NaiveTime::from_hms_opt(9, 0, 0).expect("time")),
            "Drink water",
            Local::now(),
        );

        upsert(record.clone()).expect("save reminder");
        let loaded = get("r1").expect("load reminder").expect("stored reminder");
        assert_eq!(loaded.id, record.id);

        remove("r1").expect("remove reminder");
        assert!(get("r1").expect("reload store").is_none());
        std::fs::remove_dir_all(temp).expect("cleanup temp xdg data");
        std::env::remove_var("XDG_DATA_HOME");
    }

    #[test]
    fn reminder_store_lists_records_in_created_order() {
        let _guard = env_lock().lock().expect("env lock poisoned");
        let temp = std::env::temp_dir().join(format!("tcui-reminders-list-{}", rand::random::<u64>()));
        std::fs::create_dir_all(&temp).expect("temp xdg data");
        std::env::set_var("XDG_DATA_HOME", &temp);
        let first = ReminderRecord::new_test(
            "a1",
            crate::reminders::ReminderSchedule::Daily(NaiveTime::from_hms_opt(9, 0, 0).expect("time")),
            "First",
            Local::now() - chrono::TimeDelta::minutes(5),
        );
        let second = ReminderRecord::new_test(
            "b2",
            crate::reminders::ReminderSchedule::Daily(NaiveTime::from_hms_opt(10, 0, 0).expect("time")),
            "Second",
            Local::now(),
        );

        upsert(second).expect("save second reminder");
        upsert(first).expect("save first reminder");

        let listed = list().expect("list reminders");
        assert_eq!(listed.iter().map(|record| record.id.as_str()).collect::<Vec<_>>(), ["a1", "b2"]);
        std::fs::remove_dir_all(temp).expect("cleanup temp xdg data");
        std::env::remove_var("XDG_DATA_HOME");
    }
}
