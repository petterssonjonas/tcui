use std::ffi::OsStr;
use std::fs::File;
use std::io::{self, Read};
use std::path::{Path, PathBuf};

use cap_std::fs::Dir;
use color_eyre::eyre::{eyre, WrapErr};
use rustix::fs::{fstat, openat, statat, AtFlags, Mode, OFlags};
use serde::{Deserialize, Serialize};

use super::execution::Cancellation;
use super::{ReminderError, ReminderRecord};

const STORE_FILE: &str = "reminders.toml";
const LOCK_FILE: &str = ".reminders.lock";

#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(default)]
struct ReminderStoreFile {
    reminders: Vec<ReminderRecord>,
}

#[cfg(test)]
pub(super) fn upsert(record: ReminderRecord) -> Result<(), ReminderError> {
    upsert_cancellable(record, &Cancellation::default())
}

pub(super) fn upsert_cancellable(
    record: ReminderRecord,
    cancellation: &Cancellation,
) -> Result<(), ReminderError> {
    let store = LockedStore::open(&store_root(dirs::data_dir())?)?;
    // The stable metadata lock may have blocked after the caller cancelled.
    cancellation.check()?;
    store.upsert(record, cancellation)
}

pub(super) fn get(id: &str) -> Result<Option<ReminderRecord>, ReminderError> {
    let store = LockedStore::open(&store_root(dirs::data_dir())?)?;
    Ok(store
        .load()?
        .reminders
        .into_iter()
        .find(|record| record.id == id))
}

pub(super) fn remove(id: &str) -> Result<(), ReminderError> {
    let store = LockedStore::open(&store_root(dirs::data_dir())?)?;
    store.remove(id)
}

pub(super) fn list() -> Result<Vec<ReminderRecord>, ReminderError> {
    let store = LockedStore::open(&store_root(dirs::data_dir())?)?;
    let mut reminders = store.load()?.reminders;
    reminders.sort_by(|left, right| left.created_at.cmp(&right.created_at));
    Ok(reminders)
}

pub(super) fn message_key() -> color_eyre::Result<crate::storage::crypto::SharedKey> {
    use crate::storage::crypto::SharedKey;

    let root = store_root(dirs::data_dir())?;
    let store = LockedStore::open(&root)?;
    let path = root.join("reminder.key");
    if path.try_exists()? {
        return Ok(SharedKey::load_from_path(&path)?);
    }
    let records = store.load()?.reminders;
    if records
        .iter()
        .any(|record| record.message.starts_with("enc:v1:"))
    {
        let legacy = root.join("chat.key");
        let metadata = std::fs::symlink_metadata(&legacy)
            .wrap_err("Existing reminders require reminder.key or their original chat.key; restore a matching key before retrying")?;
        if !metadata.file_type().is_file() {
            return Err(eyre!("The legacy reminder key must be a regular file"));
        }
        let key = SharedKey::load_from_path(&legacy)?;
        for record in &records {
            let _plaintext = zeroize::Zeroizing::new(
                crate::storage::crypto::decrypt_shared_text_with_key(&key, &record.message)
                    .wrap_err("chat.key cannot unlock existing reminders; restore their matching reminder.key or original chat.key")?,
            );
        }
        // Keep existing ciphertext readable without overwriting or rotating its key.
        std::fs::hard_link(&legacy, &path)?;
        crate::storage::paths::set_unix_mode(&path, 0o400)?;
        File::open(&root)?.sync_all()?;
        return Ok(key);
    }
    Ok(SharedKey::create_at(&path)?)
}

fn store_root(base: Option<PathBuf>) -> io::Result<PathBuf> {
    let base = base.ok_or_else(|| io::Error::other("cannot determine reminder data directory"))?;
    if !base.is_absolute() {
        return Err(io::Error::other("reminder data directory must be absolute"));
    }
    Ok(base.join("tcui"))
}

// All cooperating readers and writers hold this stable lock. A writer reloads
// under the lock, changes only its record, and publishes before releasing it.
// Different IDs therefore merge; for the same ID, lock acquisition order wins.
// systemd unit operations are outside this metadata-only transaction.
struct LockedStore {
    directory: Dir,
    _lock: File,
}

impl LockedStore {
    fn open(root: &Path) -> Result<Self, ReminderError> {
        let directory = crate::protection::open_directory(root, true).map_err(private_io_error)?;
        let descriptor = openat(
            &directory,
            LOCK_FILE,
            OFlags::RDWR | OFlags::CREATE | OFlags::NOFOLLOW | OFlags::NONBLOCK | OFlags::CLOEXEC,
            Mode::from_bits_truncate(0o600),
        )
        .map_err(io::Error::from)?;
        let lock = File::from(descriptor);
        let opened = fstat(&lock).map_err(io::Error::from)?;
        if !lock.metadata()?.is_file() || opened.st_nlink != 1 {
            return Err(io::Error::other("reminder lock is not a private regular file").into());
        }
        File::lock(&lock)?;
        let current =
            statat(&directory, LOCK_FILE, AtFlags::SYMLINK_NOFOLLOW).map_err(io::Error::from)?;
        if opened.st_dev != current.st_dev || opened.st_ino != current.st_ino {
            return Err(io::Error::other("reminder lock changed while opening it").into());
        }
        Ok(Self {
            directory,
            _lock: lock,
        })
    }

    fn load(&self) -> Result<ReminderStoreFile, ReminderError> {
        let descriptor = match openat(
            &self.directory,
            STORE_FILE,
            OFlags::RDONLY | OFlags::NOFOLLOW | OFlags::NONBLOCK | OFlags::CLOEXEC,
            Mode::empty(),
        ) {
            Ok(descriptor) => descriptor,
            Err(rustix::io::Errno::NOENT) => return Ok(ReminderStoreFile::default()),
            Err(error) => return Err(io::Error::from(error).into()),
        };
        let mut file = File::from(descriptor);
        if !file.metadata()?.is_file() {
            return Err(io::Error::other("reminder metadata is not a regular file").into());
        }
        let mut content = String::new();
        file.read_to_string(&mut content)?;
        Ok(toml::from_str(&content)?)
    }

    fn upsert(
        &self,
        record: ReminderRecord,
        cancellation: &Cancellation,
    ) -> Result<(), ReminderError> {
        let mut file = self.load()?;
        file.reminders.retain(|existing| existing.id != record.id);
        file.reminders.push(record);
        self.save(&file, Some(cancellation))
    }

    fn remove(&self, id: &str) -> Result<(), ReminderError> {
        let mut file = self.load()?;
        file.reminders.retain(|record| record.id != id);
        self.save(&file, None)
    }

    fn save(
        &self,
        file: &ReminderStoreFile,
        cancellation: Option<&Cancellation>,
    ) -> Result<(), ReminderError> {
        let content = toml::to_string_pretty(file)?;
        if let Some(cancellation) = cancellation {
            cancellation.check()?;
        }
        crate::config::app_config::replace_private_file(
            &self.directory,
            OsStr::new(STORE_FILE),
            content.as_bytes(),
        )
        .map_err(private_io_error)?;
        Ok(())
    }
}

fn private_io_error(error: color_eyre::Report) -> io::Error {
    io::Error::other(error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::app_config::{inject_private_file_failure, TestFailurePoint};
    use chrono::{Local, NaiveTime};
    use std::sync::Barrier;

    struct DataHome {
        root: PathBuf,
        previous: Option<std::ffi::OsString>,
    }

    impl DataHome {
        fn new() -> Self {
            let root = std::env::temp_dir()
                .join(format!("tcui-reminders-{:032x}", rand::random::<u128>()));
            let previous = std::env::var_os("XDG_DATA_HOME");
            std::env::set_var("XDG_DATA_HOME", &root);
            Self { root, previous }
        }

        fn store_root(&self) -> PathBuf {
            self.root.join("tcui")
        }

        fn path(&self) -> PathBuf {
            self.store_root().join(STORE_FILE)
        }
    }

    impl Drop for DataHome {
        fn drop(&mut self) {
            match self.previous.take() {
                Some(previous) => std::env::set_var("XDG_DATA_HOME", previous),
                None => std::env::remove_var("XDG_DATA_HOME"),
            }
            let _ = std::fs::remove_dir_all(&self.root);
        }
    }

    fn record(id: &str) -> ReminderRecord {
        ReminderRecord {
            id: id.to_string(),
            display_schedule: "daily 09:00".to_string(),
            recurring: true,
            message: format!("Dummy legacy message for {id}"),
            unit: format!("tcui-reminder-{id}"),
            created_at: "2026-10-24T09:00:00+00:00".to_string(),
            pushover: false,
        }
    }

    #[test]
    fn reminder_store_round_trip() {
        let _guard = crate::test_support::env_lock().lock().unwrap();
        let _home = DataHome::new();
        let record = ReminderRecord::new_test(
            "r1",
            crate::reminders::ReminderSchedule::Daily(
                NaiveTime::from_hms_opt(9, 0, 0).expect("time"),
            ),
            "Drink water",
            Local::now(),
        );

        upsert(record.clone()).expect("save reminder");
        assert_eq!(get("r1").unwrap().unwrap(), record);
        remove("r1").expect("remove reminder");
        assert!(get("r1").expect("reload store").is_none());
    }

    #[test]
    fn reminder_store_lists_records_in_created_order() {
        let _guard = crate::test_support::env_lock().lock().unwrap();
        let _home = DataHome::new();
        let first = record("a1");
        let mut second = record("b2");
        second.created_at = "2026-10-24T10:00:00+00:00".to_string();

        upsert(second).unwrap();
        upsert(first).unwrap();
        assert_eq!(
            list()
                .unwrap()
                .iter()
                .map(|record| record.id.as_str())
                .collect::<Vec<_>>(),
            ["a1", "b2"]
        );
    }

    #[test]
    fn failed_metadata_publication_retains_exact_prior_schedules() {
        let _guard = crate::test_support::env_lock().lock().unwrap();
        let home = DataHome::new();
        let original_record = record("retained");
        upsert(original_record.clone()).unwrap();
        let original = std::fs::read(home.path()).unwrap();

        for point in [
            TestFailurePoint::Sync,
            TestFailurePoint::Rename,
            TestFailurePoint::ParentSync,
        ] {
            inject_private_file_failure(point);
            assert!(upsert(record("not-committed")).is_err());
            assert_eq!(std::fs::read(home.path()).unwrap(), original);
            assert_eq!(
                list().unwrap().as_slice(),
                std::slice::from_ref(&original_record)
            );
            inject_private_file_failure(point);
            assert!(remove("retained").is_err());
            assert_eq!(std::fs::read(home.path()).unwrap(), original);
        }
        let mut names: Vec<_> = std::fs::read_dir(home.store_root())
            .unwrap()
            .map(|entry| entry.unwrap().file_name())
            .collect();
        names.sort();
        assert_eq!(
            names,
            [
                std::ffi::OsString::from(LOCK_FILE),
                std::ffi::OsString::from(STORE_FILE)
            ]
        );
    }

    #[test]
    fn competing_updates_to_different_records_merge_under_the_stable_lock() {
        let _guard = crate::test_support::env_lock().lock().unwrap();
        let home = DataHome::new();
        let retained = record("retained");
        upsert(retained.clone()).unwrap();
        upsert(record("remove-me")).unwrap();
        let held = LockedStore::open(&home.store_root()).unwrap();
        let competing = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open(home.store_root().join(LOCK_FILE))
            .unwrap();
        assert!(matches!(
            competing.try_lock(),
            Err(std::fs::TryLockError::WouldBlock)
        ));
        let barrier = Barrier::new(4);
        std::thread::scope(|scope| {
            let first = scope.spawn(|| {
                barrier.wait();
                upsert(record("first"))
            });
            let second = scope.spawn(|| {
                barrier.wait();
                upsert(record("second"))
            });
            let removed = scope.spawn(|| {
                barrier.wait();
                remove("remove-me")
            });
            barrier.wait();
            drop(held);
            first.join().unwrap().unwrap();
            second.join().unwrap().unwrap();
            removed.join().unwrap().unwrap();
        });
        let mut records = list().unwrap();
        records.sort_by(|left, right| left.id.cmp(&right.id));
        assert_eq!(records, [record("first"), retained, record("second")]);
    }

    #[test]
    fn malformed_metadata_is_not_replaced_with_an_empty_schedule() {
        let _guard = crate::test_support::env_lock().lock().unwrap();
        let home = DataHome::new();
        std::fs::create_dir_all(home.store_root()).unwrap();
        let original = b"[[reminders]\ninvalid dummy metadata";
        std::fs::write(home.path(), original).unwrap();
        assert!(upsert(record("new")).is_err());
        assert!(remove("old").is_err());
        assert_eq!(std::fs::read(home.path()).unwrap(), original);
    }

    #[cfg(unix)]
    #[test]
    fn reminder_metadata_and_lock_links_never_modify_their_sentinel() {
        use std::os::unix::fs::symlink;

        let _guard = crate::test_support::env_lock().lock().unwrap();
        for leaf in [STORE_FILE, LOCK_FILE] {
            let home = DataHome::new();
            std::fs::create_dir_all(home.store_root()).unwrap();
            let sentinel = home.root.join("sentinel");
            std::fs::write(&sentinel, b"private dummy sentinel").unwrap();
            let link = home.store_root().join(leaf);
            symlink(&sentinel, &link).unwrap();
            assert!(upsert(record("new")).is_err());
            assert!(remove("old").is_err());
            assert!(list().is_err());
            assert_eq!(std::fs::read(&sentinel).unwrap(), b"private dummy sentinel");
            assert!(std::fs::symlink_metadata(link).unwrap().is_symlink());
        }
    }

    #[cfg(unix)]
    #[test]
    fn reminder_metadata_and_lock_are_private_and_parent_links_are_rejected() {
        use std::os::unix::fs::{symlink, PermissionsExt};

        let _guard = crate::test_support::env_lock().lock().unwrap();
        let home = DataHome::new();
        upsert(record("private")).unwrap();
        for leaf in [STORE_FILE, LOCK_FILE] {
            assert_eq!(
                std::fs::metadata(home.store_root().join(leaf))
                    .unwrap()
                    .permissions()
                    .mode()
                    & 0o777,
                0o600
            );
        }
        let outside = home.root.join("outside");
        std::fs::create_dir(&outside).unwrap();
        let alias = home.root.join("alias");
        symlink(&outside, &alias).unwrap();
        assert!(LockedStore::open(&alias).is_err());
        assert!(std::fs::read_dir(outside).unwrap().next().is_none());
    }

    #[test]
    fn missing_or_relative_reminder_roots_return_errors() {
        assert!(store_root(None).is_err());
        assert!(store_root(Some(PathBuf::from("relative-data"))).is_err());
    }
}
