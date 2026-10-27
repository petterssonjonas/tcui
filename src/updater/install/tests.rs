#![cfg(unix)]

use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

use super::{is_executable, is_system_path, path_notice, should_replace_current_binary};

struct Fixture(PathBuf);

impl Fixture {
    fn new() -> Self {
        let path =
            std::env::temp_dir().join(format!("tcui-update-test-{:032x}", rand::random::<u128>()));
        std::fs::create_dir(&path).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o700)).unwrap();
        Self(path)
    }

    fn binary(&self, relative: &str) -> PathBuf {
        let path = self.0.join(relative);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, b"existing installation").unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
        path
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[test]
fn owned_regular_local_binary_can_be_replaced_without_leaving_a_probe() {
    let fixture = Fixture::new();
    let binary = fixture.binary("bin/tcui");
    assert!(should_replace_current_binary(&binary));
    assert_eq!(
        std::fs::read_dir(binary.parent().unwrap()).unwrap().count(),
        1
    );
    assert_eq!(std::fs::read(binary).unwrap(), b"existing installation");
}

#[test]
fn development_and_managed_binaries_are_not_replaced_even_if_owned_and_writable() {
    let fixture = Fixture::new();
    for relative in [
        "project/target/release/tcui",
        ".linuxbrew/bin/tcui",
        ".guix-profile/bin/tcui",
        "repo/bin/tcui",
    ] {
        let binary = fixture.binary(relative);
        if relative == "repo/bin/tcui" {
            std::fs::write(fixture.0.join("repo/Cargo.toml"), b"[package]").unwrap();
        }
        assert!(
            !should_replace_current_binary(&binary),
            "unsafe target: {relative}"
        );
        assert_eq!(std::fs::read(binary).unwrap(), b"existing installation");
    }
    for system in [
        "/usr/bin/tcui",
        "/usr/local/bin/tcui",
        "/opt/tcui/bin/tcui",
        "/bin/tcui",
        "/gnu/store/release-tcui/bin/tcui",
    ] {
        assert!(is_system_path(Path::new(system)));
    }
}

#[test]
fn symlink_and_untrusted_writable_binary_are_not_replaced() {
    let fixture = Fixture::new();
    let binary = fixture.binary("bin/tcui");
    std::fs::set_permissions(&binary, std::fs::Permissions::from_mode(0o777)).unwrap();
    assert!(!should_replace_current_binary(&binary));
    std::fs::remove_file(&binary).unwrap();
    let original = fixture.binary("original");
    std::os::unix::fs::symlink(original, &binary).unwrap();
    assert!(!should_replace_current_binary(&binary));
}

#[test]
fn output_identifies_earlier_path_binary_instead_of_claiming_it_was_upgraded() {
    let fixture = Fixture::new();
    let old = fixture.binary("old/tcui");
    let new = fixture.binary("new/tcui");
    let path = std::env::join_paths([old.parent().unwrap(), new.parent().unwrap()]).unwrap();
    let notice = path_notice(&new, Some(&path));
    assert!(notice.contains(&old.display().to_string()));
    assert!(notice.contains(&new.display().to_string()));
    let path = std::env::join_paths([new.parent().unwrap(), old.parent().unwrap()]).unwrap();
    assert!(!path_notice(&new, Some(&path)).contains(&old.display().to_string()));
}

#[test]
fn path_lookup_skips_files_the_current_user_cannot_execute() {
    if rustix::process::geteuid().is_root() {
        return;
    }
    let fixture = Fixture::new();
    let inaccessible = fixture.binary("old/tcui");
    std::fs::set_permissions(&inaccessible, std::fs::Permissions::from_mode(0o641)).unwrap();
    let installed = fixture.binary("new/tcui");
    let path = std::env::join_paths([inaccessible.parent().unwrap(), installed.parent().unwrap()])
        .unwrap();

    assert!(!is_executable(&inaccessible));
    assert!(!path_notice(&installed, Some(&path)).contains(&inaccessible.display().to_string()));
}
