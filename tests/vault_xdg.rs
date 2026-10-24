#![cfg(unix)]

use std::error::Error;
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::Command;

const SCENARIOS: [(&str, &str); 11] = [
    (
        "TOML migration discards external selection without touching external content",
        "config::app_config::vault_config_migration::migration_cases::vault_config_migration_discards_external_selection_without_importing_or_mutating_files",
    ),
    (
        "JSON migration preserves explicit Saved files and copies them into native Vault",
        "config::app_config::vault_config_migration::migration_cases::vault_config_migration_json_keeps_original_and_explicit_saved_directory",
    ),
    (
        "migration preserves visibility and existing native Vault content",
        "config::app_config::vault_config_migration::migration_cases::vault_config_migration_preserves_visibility_and_native_content",
    ),
    (
        "native Vault persistence and workspace-tab ephemerality",
        "app::vault_restart_tests::vault_restart_preserves_native_content_but_not_workspace_tabs",
    ),
    (
        "Saved-file link restart",
        "storage::chat_files::tests::chat_saved_file_store_persistence::chat_saved_file_store_restart_preserves_link_and_bytes",
    ),
    (
        "per-process Vault lock",
        "vault::write::tests::vault_confined_write_lock_contention_is_cross_process",
    ),
    (
        "move recovery after restart",
        "vault::vault_move_recovery::vault_move_recovery_restart_and_second_rerun_are_idempotent",
    ),
    (
        "future Chat link manifest fails closed",
        "storage::chat_files::tests::chat_saved_file_store_reconciliation::chat_saved_file_store_preserves_future_manifest_without_rewrite",
    ),
    (
        "future move journal fails closed",
        "vault::vault_move_execute::move_execution_unknown_journal_version_blocks_and_retains_all_data",
    ),
    (
        "read traversal stays confined",
        "vault::fs::tests::vault_confined_read_rejects_outside_root_traversal_before_io",
    ),
    (
        "write symlink escape leaves outside sentinel unchanged",
        "vault::write::tests::vault_confined_write_symlink_escape_never_reaches_outside_sentinel",
    ),
];

struct OutsideSentinel {
    path: PathBuf,
}

impl Drop for OutsideSentinel {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

fn required_environment_path(name: &str) -> Result<PathBuf, Box<dyn Error>> {
    std::env::var_os(name)
        .map(PathBuf::from)
        .ok_or_else(|| format!("{name} must be set for isolated verification").into())
}

fn verify_root() -> Result<PathBuf, Box<dyn Error>> {
    let paths = [
        required_environment_path("XDG_CONFIG_HOME")?,
        required_environment_path("XDG_DATA_HOME")?,
        required_environment_path("XDG_CACHE_HOME")?,
        required_environment_path("HOME")?,
    ];
    let root = paths[0]
        .parent()
        .ok_or("XDG_CONFIG_HOME must have a parent")?
        .to_path_buf();
    if paths.iter().any(|path| path.parent() != Some(&root)) {
        return Err("all isolated XDG and HOME directories must share one root".into());
    }
    Ok(root)
}

fn run_scenario(
    cargo: &OsString,
    root: &Path,
    temporary: &Path,
    label: &str,
    test_name: &str,
) -> Result<(), Box<dyn Error>> {
    println!("\n=== {label} ===\n{test_name}");
    let output = Command::new(cargo)
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .env("TMPDIR", temporary)
        .env("TCUI_VAULT_VERIFY_ROOT", root)
        .args(["test", "--all-features", "--bin", "tcui", "--"])
        .arg("--exact")
        .arg(test_name)
        .args(["--nocapture", "--test-threads=1"])
        .output()?;
    let stdout = String::from_utf8(output.stdout)?;
    let stderr = String::from_utf8(output.stderr)?;
    print!("{stdout}");
    eprint!("{stderr}");
    if !output.status.success() {
        return Err(format!("scenario failed: {label}").into());
    }
    if !stdout.contains("1 passed") {
        return Err(format!("scenario did not execute exactly one passing test: {label}").into());
    }
    Ok(())
}

#[test]
fn vault_xdg_verifies_restart_recovery_and_confinement() -> Result<(), Box<dyn Error>> {
    // Given
    let root = verify_root()?;
    let temporary = root.join("tmp");
    std::fs::create_dir_all(&temporary)?;
    let sentinel_path = root
        .parent()
        .ok_or("verification root must have a parent")?
        .join(format!(
            ".{}-outside-sentinel",
            root.file_name()
                .ok_or("verification root must have a name")?
                .to_string_lossy()
        ));
    let sentinel_bytes = b"vault-outside-root-sentinel\n";
    std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&sentinel_path)
        .and_then(|mut file| std::io::Write::write_all(&mut file, sentinel_bytes))?;
    let _sentinel = OutsideSentinel {
        path: sentinel_path.clone(),
    };
    let cargo = std::env::var_os("CARGO").unwrap_or_else(|| OsString::from("cargo"));
    println!("VERIFY_ROOT={}", root.display());
    println!("OUTSIDE_SENTINEL={}", sentinel_path.display());

    // When
    for (label, test_name) in SCENARIOS {
        run_scenario(&cargo, &root, &temporary, label, test_name)?;
    }

    // Then
    assert_eq!(std::fs::read(&sentinel_path)?, sentinel_bytes);
    println!("verified_scenarios={}", SCENARIOS.len());
    println!("outside_root_sentinel=unchanged");
    Ok(())
}
