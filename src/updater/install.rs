use std::ffi::{OsStr, OsString};
use std::path::Path;
use std::process::{Command, Stdio};

use color_eyre::{eyre::eyre, Result};

use super::{ReleaseInfo, RELEASE_REPO};

// Compile the reviewed installer into tcui. Never execute a downloaded script.
const INSTALLER: &str = include_str!("../../scripts/install.sh");

pub(super) async fn install_release(release: &ReleaseInfo) -> Result<String> {
    let directory = tokio::task::spawn_blocking(install_directory).await??;
    let target = Path::new(&directory).join("tcui");
    let current = std::env::current_exe()?;
    let original_notice = if same_path(&current, &target) {
        "The safe user-owned binary was replaced in place.".to_owned()
    } else {
        format!("The original installation at {} was not changed. System, package-managed, non-writable and development installations use a separate destination.", current.display())
    };
    let output = tokio::process::Command::new("bash")
        .args(["--noprofile", "--norc", "-c", INSTALLER, "tcui-updater"])
        .env_remove("BASH_ENV")
        .env_remove("ENV")
        .env_remove("TCUI_VERSION")
        .env("TCUI_REPO", RELEASE_REPO)
        .env("TCUI_RELEASE_TAG", &release.tag)
        .env("TCUI_BIN_DIR", &directory)
        .stdin(Stdio::null())
        .kill_on_drop(true)
        .output()
        .await?;
    if !output.status.success() {
        return Err(eyre!(
            "Could not install release {} at {} ({}): {}",
            release.tag,
            target.display(),
            output.status,
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    Ok(format!(
        "Installed tcui {} at {}. Configuration, chats and credentials were not changed.\n{}\n{}",
        release.version,
        target.display(),
        original_notice,
        path_notice(&target, std::env::var_os("PATH").as_deref())
    ))
}

fn install_directory() -> Result<OsString> {
    if let Some(directory) = std::env::var_os("TCUI_BIN_DIR") {
        let target = Path::new(&directory).join("tcui");
        if is_system_path(&target) || package_owns(&target) {
            return Err(eyre!(
                "Refusing to replace a system or package-managed binary at {}. Set TCUI_BIN_DIR to a user-local directory instead.",
                target.display()
            ));
        }
        // Preserve the exact spelling: the installer must reject trailing /,
        // empty components, relative paths, symlinks and non-owned targets.
        return Ok(directory);
    }
    let current = std::env::current_exe()?;
    if should_replace_current_binary(&current) {
        if let Some(parent) = current.parent() {
            return Ok(parent.as_os_str().to_owned());
        }
    }
    let home = dirs::home_dir()
        .ok_or_else(|| eyre!("Could not resolve home directory for upgrade target"))?;
    let target = home.join(".local/bin/tcui");
    if is_system_path(&target) || package_owns(&target) {
        return Err(eyre!(
            "The user-local target {} is system or package-managed. Set TCUI_BIN_DIR to another user-local directory.",
            target.display()
        ));
    }
    Ok(home.join(".local/bin").into_os_string())
}

fn is_system_path(path: &Path) -> bool {
    [
        "/usr", "/bin", "/sbin", "/lib", "/lib64", "/etc", "/opt", "/var", "/nix", "/gnu", "/snap",
    ]
    .iter()
    .any(|prefix| path.starts_with(prefix))
        || path.components().any(|component| {
            matches!(
                component.as_os_str().to_str(),
                Some(".linuxbrew" | "Cellar" | ".nix-profile" | ".guix-profile")
            )
        })
}

fn package_owns(path: &Path) -> bool {
    if !path.exists() {
        return false;
    }
    [("dpkg-query", "-S"), ("rpm", "-qf"), ("pacman", "-Qo")]
        .iter()
        .any(|(program, option)| {
            Command::new(program)
                .arg(option)
                .arg(path)
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status()
                .is_ok_and(|status| status.success())
        })
}

fn looks_like_dev_binary(path: &Path) -> bool {
    path.ancestors().any(|ancestor| {
        ancestor.file_name() == Some(OsStr::new("target"))
            || ancestor.join(".git").exists()
            || ancestor.join("Cargo.toml").is_file()
    })
}

#[cfg(unix)]
fn should_replace_current_binary(current: &Path) -> bool {
    use std::os::unix::fs::MetadataExt;

    if current.file_name() != Some(OsStr::new("tcui"))
        || is_system_path(current)
        || looks_like_dev_binary(current)
        || package_owns(current)
    {
        return false;
    }
    let Some(parent) = current.parent() else {
        return false;
    };
    let (Ok(binary), Ok(directory)) = (
        std::fs::symlink_metadata(current),
        std::fs::symlink_metadata(parent),
    ) else {
        return false;
    };
    let owner = rustix::process::geteuid().as_raw();
    binary.is_file()
        && binary.uid() == owner
        && binary.mode() & 0o022 == 0
        && directory.is_dir()
        && directory.uid() == owner
        && directory.mode() & 0o022 == 0
        && directory_is_writable(parent)
}

#[cfg(not(unix))]
fn should_replace_current_binary(_current: &Path) -> bool {
    false
}

#[cfg(unix)]
fn directory_is_writable(path: &Path) -> bool {
    use std::os::unix::fs::OpenOptionsExt;

    let probe = path.join(format!(".tcui-write-test-{:032x}", rand::random::<u128>()));
    match std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(&probe)
    {
        Ok(file) => {
            drop(file);
            std::fs::remove_file(probe).is_ok()
        }
        Err(_) => false,
    }
}

fn path_notice(target: &Path, path: Option<&OsStr>) -> String {
    let first = path.and_then(|path| {
        std::env::split_paths(path)
            .map(|directory| directory.join("tcui"))
            .find(|candidate| is_executable(candidate))
    });
    match first {
        Some(first) if same_path(&first, target) => {
            "The next `tcui` command on this PATH will use this installation. Restart any running tcui process; if your shell cached another path, run `hash -r` or `rehash`.".to_owned()
        }
        Some(first) => format!(
            "PATH warning: `tcui` still resolves to {}, not the new installation. Run {} directly, or put {} before the old directory in PATH, then run `hash -r` or `rehash`.",
            first.display(), target.display(), target.parent().unwrap_or(target).display()
        ),
        None => format!(
            "PATH warning: `tcui` is not on PATH. Run {} directly, or add {} to PATH.",
            target.display(), target.parent().unwrap_or(target).display()
        ),
    }
}

fn same_path(left: &Path, right: &Path) -> bool {
    match (left.canonicalize(), right.canonicalize()) {
        (Ok(left), Ok(right)) => left == right,
        _ => left == right,
    }
}

#[cfg(unix)]
fn is_executable(path: &Path) -> bool {
    use rustix::fs::{accessat, Access, AtFlags, CWD};

    std::fs::metadata(path).is_ok_and(|metadata| metadata.is_file())
        && accessat(CWD, path, Access::EXEC_OK, AtFlags::EACCESS).is_ok()
}

#[cfg(not(unix))]
fn is_executable(path: &Path) -> bool {
    path.is_file()
}

#[cfg(test)]
mod tests;
