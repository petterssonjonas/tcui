use std::fs::File;
use std::io::{self, Read, Write};
use std::path::{Component, Path};

use cap_std::fs::Dir;
use rustix::fs::{mkdirat, openat, Mode, OFlags};

pub(super) fn directory(path: &Path, create: bool) -> io::Result<Dir> {
    let mut directory = Dir::open_ambient_dir(
        if path.is_absolute() { "/" } else { "." },
        cap_std::ambient_authority(),
    )?;
    for component in path.components() {
        let name = match component {
            Component::RootDir | Component::CurDir => continue,
            Component::Normal(name) => name,
            _ => {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "unsafe storage parent",
                ))
            }
        };
        let flags = OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC;
        let descriptor = match openat(&directory, name, flags, Mode::empty()) {
            Ok(descriptor) => descriptor,
            Err(rustix::io::Errno::NOENT) if create => {
                match mkdirat(&directory, name, Mode::from_bits_truncate(0o700)) {
                    Ok(()) => rustix::fs::fsync(&directory)?,
                    Err(rustix::io::Errno::EXIST) => {}
                    Err(error) => return Err(error.into()),
                }
                openat(&directory, name, flags, Mode::empty())?
            }
            Err(error) => return Err(error.into()),
        };
        directory = Dir::from(descriptor);
    }
    Ok(directory)
}

pub(super) fn read_regular(path: &Path) -> io::Result<Vec<u8>> {
    let parent = path
        .parent()
        .ok_or_else(|| io::Error::other("storage path has no parent"))?;
    let name = path
        .file_name()
        .ok_or_else(|| io::Error::other("storage path has no filename"))?;
    let directory = directory(parent, false)?;
    let descriptor = openat(
        &directory,
        name,
        OFlags::RDONLY | OFlags::NOFOLLOW | OFlags::NONBLOCK | OFlags::CLOEXEC,
        Mode::empty(),
    )?;
    let mut file = File::from(descriptor);
    if !file.metadata()?.is_file() {
        return Err(io::Error::other(
            "protected storage path is not a regular file",
        ));
    }
    let mut bytes = Vec::new();
    file.read_to_end(&mut bytes)?;
    Ok(bytes)
}

pub(super) fn atomic_write(path: &Path, bytes: &[u8]) -> io::Result<()> {
    atomic_write_impl(path, bytes, true)
}

pub(super) fn atomic_write_new(path: &Path, bytes: &[u8]) -> io::Result<()> {
    atomic_write_impl(path, bytes, false)
}

fn atomic_write_impl(path: &Path, bytes: &[u8], replace: bool) -> io::Result<()> {
    let parent = path
        .parent()
        .ok_or_else(|| io::Error::other("storage path has no parent"))?;
    let name = path
        .file_name()
        .ok_or_else(|| io::Error::other("storage path has no filename"))?;
    let directory = directory(parent, true)?;
    match directory.symlink_metadata(name) {
        Ok(_) if !replace => {
            return Err(io::Error::new(
                io::ErrorKind::AlreadyExists,
                "protected storage destination already exists",
            ));
        }
        Ok(metadata) if !metadata.is_file() || metadata.is_symlink() => {
            return Err(io::Error::other(
                "refusing to replace a non-regular protected file",
            ));
        }
        Ok(_) => {}
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(error) => return Err(error),
    }
    let temporary = format!(".tcui-protection-{:032x}.tmp", rand::random::<u128>());
    let descriptor = openat(
        &directory,
        temporary.as_str(),
        OFlags::WRONLY | OFlags::CREATE | OFlags::EXCL | OFlags::NOFOLLOW | OFlags::CLOEXEC,
        Mode::from_bits_truncate(0o600),
    )?;
    let mut file = File::from(descriptor);
    let result = (|| {
        file.write_all(bytes)?;
        file.sync_all()?;
        if replace {
            directory.rename(&temporary, &directory, name)?;
        } else {
            #[cfg(any(target_os = "linux", target_os = "android"))]
            rustix::fs::renameat_with(
                &directory,
                temporary.as_str(),
                &directory,
                name,
                rustix::fs::RenameFlags::NOREPLACE,
            )?;
            #[cfg(not(any(target_os = "linux", target_os = "android")))]
            {
                directory.hard_link(&temporary, &directory, name)?;
                directory.remove_file(&temporary)?;
            }
        }
        rustix::fs::fsync(&directory)?;
        Ok(())
    })();
    if result.is_err() {
        let _ = directory.remove_file(&temporary);
    }
    result
}

pub(super) fn migration_lock() -> io::Result<File> {
    lock_file(true)
}

pub(super) fn session_lock() -> io::Result<File> {
    lock_file(false)
}

fn lock_file(exclusive: bool) -> io::Result<File> {
    let root = super::root().map_err(|error| io::Error::other(error.to_string()))?;
    let directory = directory(&root, true)?;
    let descriptor = openat(
        &directory,
        "protection.lock",
        OFlags::RDWR | OFlags::CREATE | OFlags::NOFOLLOW | OFlags::CLOEXEC,
        Mode::from_bits_truncate(0o600),
    )?;
    let file = File::from(descriptor);
    if !file.metadata()?.is_file() {
        return Err(io::Error::other("protection lock is not a regular file"));
    }
    let result = if exclusive {
        file.try_lock()
    } else {
        file.try_lock_shared()
    };
    result.map_err(|error| io::Error::other(format!("Protection settings are busy: {error}")))?;
    Ok(file)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;
    use std::sync::Barrier;

    #[test]
    fn concurrent_new_publications_never_replace_the_winner() {
        let root = std::env::temp_dir().join(format!(
            "tcui-export-publication-{:032x}",
            rand::random::<u128>()
        ));
        directory(&root, true).unwrap();
        let destination = root.join("export.md");
        let barrier = Barrier::new(2);
        let results = std::thread::scope(|scope| {
            let first = scope.spawn(|| {
                barrier.wait();
                atomic_write_new(&destination, b"first complete export")
            });
            let second = scope.spawn(|| {
                barrier.wait();
                atomic_write_new(&destination, b"second complete export")
            });
            [first.join().unwrap(), second.join().unwrap()]
        });
        assert_ne!(results[0].is_ok(), results[1].is_ok());
        let expected = if results[0].is_ok() {
            b"first complete export".as_slice()
        } else {
            b"second complete export".as_slice()
        };
        assert_eq!(read_regular(&destination).unwrap(), expected);
        let error = results.into_iter().find_map(Result::err).unwrap();
        assert_eq!(error.kind(), io::ErrorKind::AlreadyExists);
        assert_eq!(
            destination.metadata().unwrap().permissions().mode() & 0o777,
            0o600
        );
        let names: Vec<_> = std::fs::read_dir(&root)
            .unwrap()
            .map(|entry| entry.unwrap().file_name())
            .collect();
        assert_eq!(names, [std::ffi::OsString::from("export.md")]);
        std::fs::remove_dir_all(root).unwrap();
    }
}
