use std::ffi::OsStr;
use std::fs::File;
use std::io::{self, Read};

use cap_std::fs::Dir;
use color_eyre::eyre::{ensure, eyre, Result};
use rustix::fs::{fstat, openat, statat, AtFlags, Mode, OFlags};
use serde::{Deserialize, Serialize};
use zeroize::Zeroizing;

use crate::config::AppConfig;
use crate::notifications::pushover::Delivery;

const MAX_JOB_BYTES: u64 = 16 * 1024;

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct PendingPing {
    delivery: Delivery,
    message: String,
    recurring: bool,
}

pub(super) fn save(
    guard: &JobGuard,
    delivery: Delivery,
    message: &str,
    recurring: bool,
    cancellation: &super::execution::Cancellation,
) -> Result<()> {
    ensure!(
        !message.is_empty() && message.chars().count() <= 1024 && !message.contains('\0'),
        "Pushover reminder messages must contain 1–1024 characters"
    );
    let job = PendingPing {
        delivery,
        message: message.to_owned(),
        recurring,
    };
    let content = Zeroizing::new(serde_json::to_vec(&job)?);
    ensure!(
        content.len() as u64 <= MAX_JOB_BYTES,
        "Pushover reminder snapshot is too large"
    );
    ensure!(
        !guard.directory.try_exists(&guard.name)?,
        "reminder job already exists"
    );
    cancellation.check()?;
    crate::config::app_config::replace_private_file(
        &guard.directory,
        OsStr::new(&guard.name),
        &content,
    )
}

pub(super) fn remove(guard: &JobGuard) -> Result<()> {
    guard.remove()
}

pub(super) async fn dispatch(id: &str) -> Result<()> {
    let guard = JobGuard::wait(id).await?;
    let Some(record) = super::store::get(id)? else {
        if guard.directory.try_exists(&guard.name)? {
            ensure_one_shot_snapshot(&guard)?;
            finish(&guard, id, &format!("tcui-reminder-{id}"))?;
            return Err(eyre!("Pushover reminder is no longer registered"));
        }
        return Ok(());
    };
    ensure!(record.pushover, "Pushover reminder is no longer registered");
    let result = async {
        let job = guard
            .read()?
            .ok_or_else(|| eyre!("Pushover reminder payload is missing"))?;
        ensure!(
            job.recurring == record.recurring,
            "Pushover reminder cadence changed"
        );
        // Only this pre-authorized notification payload is read while the vault is locked.
        let config = AppConfig::load().map_err(|_| eyre!("Cannot load reminder configuration"))?;
        crate::notifications::notify_reminder_with_delivery(&config, &job.message, &job.delivery)
            .await
    }
    .await;
    if !record.recurring {
        finish(&guard, id, &record.unit)?;
    }
    result?;
    println!("Pushover reminder delivered: {id}");
    Ok(())
}

pub(super) fn cleanup(id: &str) -> Result<()> {
    let guard = JobGuard::open(id)?;
    let record = super::store::get(id);
    match &record {
        Ok(Some(record)) if record.recurring => {
            return Err(eyre!("Recurring reminders must be explicitly cancelled"));
        }
        Ok(Some(_)) => {}
        Ok(None) if !guard.directory.try_exists(&guard.name)? => return Ok(()),
        _ => ensure_one_shot_snapshot(&guard)?,
    }
    finish(&guard, id, &format!("tcui-reminder-{id}"))
}

fn ensure_one_shot_snapshot(guard: &JobGuard) -> Result<()> {
    // Missing or unreadable metadata is not permission to delete an unknown
    // recurring job. A valid private one-shot snapshot is required.
    let job = guard
        .read()?
        .ok_or_else(|| eyre!("Cannot establish reminder cadence"))?;
    ensure!(
        !job.recurring,
        "Recurring reminders must be explicitly cancelled"
    );
    Ok(())
}

pub(super) fn finish(guard: &JobGuard, id: &str, unit: &str) -> Result<()> {
    // Try both privacy cleanup and timer disarming. Keep metadata discoverable
    // if either fails, so cancellation can retry without resending the payload.
    let payload = guard.remove();
    let timer = super::systemd::complete(unit);
    if payload.is_ok() && timer.is_ok() {
        super::store::remove(id)?;
    }
    payload?;
    timer?;
    Ok(())
}

pub(super) struct JobGuard {
    directory: Dir,
    name: String,
    _lock: File,
}

impl JobGuard {
    pub(super) fn open(id: &str) -> Result<Self> {
        Self::open_with(id, false)
    }

    pub(super) async fn wait(id: &str) -> Result<Self> {
        let id = id.to_owned();
        // A newly armed timer may fire before systemd-run acknowledges it.
        // Wait off the async executor until registration releases its guard.
        tokio::task::spawn_blocking(move || Self::open_with(&id, true)).await?
    }

    fn open_with(id: &str, wait: bool) -> Result<Self> {
        ensure!(
            !id.is_empty() && id.len() <= 64 && id.bytes().all(|byte| byte.is_ascii_alphanumeric()),
            "invalid reminder ID"
        );
        let data =
            dirs::data_dir().ok_or_else(|| eyre!("cannot determine reminder data directory"))?;
        ensure!(
            data.is_absolute(),
            "reminder data directory must be absolute"
        );
        let directory = crate::protection::open_directory(
            &data.join("tcui").join("reminder-notifications"),
            true,
        )?;
        let metadata = fstat(&directory)?;
        // SAFETY: geteuid has no arguments and does not access application memory.
        let owner = unsafe { libc::geteuid() };
        ensure!(
            metadata.st_uid == owner && metadata.st_mode & 0o077 == 0,
            "reminder notification directory must be owned by this user and mode 0700"
        );
        let lock_name = format!("{id}.lock");
        let descriptor = openat(
            &directory,
            lock_name.as_str(),
            OFlags::RDWR | OFlags::CREATE | OFlags::NOFOLLOW | OFlags::NONBLOCK | OFlags::CLOEXEC,
            Mode::from_bits_truncate(0o600),
        )?;
        let lock = File::from(descriptor);
        validate_private(&lock, owner)?;
        if wait {
            lock.lock()?;
        } else {
            lock.try_lock()
                .map_err(|error| eyre!("cannot lock reminder job: {error}"))?;
        }
        let opened = fstat(&lock)?;
        let current = statat(&directory, lock_name.as_str(), AtFlags::SYMLINK_NOFOLLOW)?;
        ensure!(
            opened.st_dev == current.st_dev && opened.st_ino == current.st_ino,
            "reminder lock changed while opening it"
        );
        Ok(Self {
            directory,
            name: format!("{id}.json"),
            _lock: lock,
        })
    }

    fn read(&self) -> Result<Option<PendingPing>> {
        let descriptor = match openat(
            &self.directory,
            self.name.as_str(),
            OFlags::RDONLY | OFlags::NOFOLLOW | OFlags::NONBLOCK | OFlags::CLOEXEC,
            Mode::empty(),
        ) {
            Ok(file) => file,
            Err(rustix::io::Errno::NOENT) => return Ok(None),
            Err(error) => return Err(error.into()),
        };
        let file = File::from(descriptor);
        validate_private(&file, fstat(&self.directory)?.st_uid)?;
        let mut content = Zeroizing::new(Vec::new());
        file.take(MAX_JOB_BYTES + 1).read_to_end(&mut content)?;
        ensure!(
            content.len() as u64 <= MAX_JOB_BYTES,
            "Pushover reminder snapshot is too large"
        );
        serde_json::from_slice(&content)
            .map(Some)
            .map_err(|_| eyre!("Pushover reminder snapshot is invalid"))
    }

    fn remove(&self) -> Result<()> {
        match self.directory.remove_file(&self.name) {
            Ok(()) => rustix::fs::fsync(&self.directory).map_err(Into::into),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(error.into()),
        }
    }
}

fn validate_private(file: &File, owner: u32) -> Result<()> {
    let metadata = file.metadata()?;
    let status = fstat(file)?;
    ensure!(
        metadata.is_file()
            && status.st_nlink == 1
            && status.st_uid == owner
            && status.st_mode & 0o077 == 0,
        "reminder snapshot and lock must be private, single-link regular files owned by this user"
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::{symlink, PermissionsExt};

    #[test]
    fn private_job_rejects_traversal_without_creating_paths() {
        for id in ["../outside", "/etc/passwd", "", "invalid id"] {
            assert!(JobGuard::open(id).is_err());
        }
    }

    #[test]
    fn job_reader_rejects_symlinks_public_files_and_oversized_content() {
        let _lock = crate::test_support::env_lock().lock().unwrap();
        let root =
            std::env::temp_dir().join(format!("tcui-ping-private-{:032x}", rand::random::<u128>()));
        let previous = std::env::var_os("XDG_DATA_HOME");
        std::env::set_var("XDG_DATA_HOME", &root);
        let id = "0123456789abcdef";
        let guard = JobGuard::open(id).unwrap();
        let directory = root.join("tcui/reminder-notifications");
        let path = directory.join(format!("{id}.json"));
        let sentinel = root.join("sentinel");
        let valid = serde_json::to_vec(&serde_json::json!({
            "delivery": {
                "program": "/usr/bin/true",
                "user_key": "uuuuuuuuuuuuuuuuuuuuuuuuuuuuuu",
                "app_key": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
            },
            "message": "private sentinel",
            "recurring": false,
        }))
        .unwrap();
        std::fs::write(&sentinel, &valid).unwrap();
        std::fs::set_permissions(&sentinel, std::fs::Permissions::from_mode(0o600)).unwrap();
        symlink(&sentinel, &path).unwrap();
        assert!(guard.read().is_err());
        std::fs::remove_file(&path).unwrap();
        std::fs::write(&path, &valid).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).unwrap();
        assert!(guard.read().is_err());
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();
        assert_eq!(guard.read().unwrap().unwrap().message, "private sentinel");
        std::fs::write(&path, vec![b' '; MAX_JOB_BYTES as usize + 1]).unwrap();
        assert!(guard.read().is_err());
        drop(guard);
        match previous {
            Some(value) => std::env::set_var("XDG_DATA_HOME", value),
            None => std::env::remove_var("XDG_DATA_HOME"),
        }
        std::fs::remove_dir_all(root).unwrap();
    }
}
