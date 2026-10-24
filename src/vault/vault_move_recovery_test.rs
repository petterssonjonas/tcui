// allow: SIZE_OK - recovery integration tests exercise durable filesystem state across restarts.

use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use crate::config::vault_profiles::{VaultCatalog, VaultProfile, VaultProfileId};
use crate::vault::move_execute::{
    inject_failure as inject_move_failure, inject_interruption as inject_move_interruption,
    MoveExecutor, TestFailurePoint as MoveFailurePoint,
};
use crate::vault::move_journal::{JournalStore, MoveJournalState, MoveOperationId};
use crate::vault::move_plan::{MovePair, MovePlan, VaultFileRef};
use crate::vault::move_recovery::{
    inject_failure as inject_recovery_failure, MoveRecoveryConfig, MoveRecoveryService,
    RecoveryFailurePoint, RecoveryStatus,
};
use crate::vault::{VaultMutationLock, VaultRelativePath, VaultWriteError};

struct Fixture {
    base: PathBuf,
    source_root: PathBuf,
    destination_root: PathBuf,
    data_root: PathBuf,
    source_id: VaultProfileId,
    destination_id: VaultProfileId,
    catalog: VaultCatalog,
}

impl Fixture {
    fn new(label: &str) -> Self {
        let base = std::env::temp_dir().join(format!(
            "tcui-vault-move-recovery-{label}-{}-{}",
            std::process::id(),
            rand::random::<u64>()
        ));
        let source_root = base.join("source");
        let destination_root = base.join("destination");
        let data_root = base.join("data").join("tcui");
        fs::create_dir_all(&source_root).expect("create source root");
        fs::create_dir_all(&destination_root).expect("create destination root");
        let source = VaultProfile::new("Source", &source_root).expect("source profile");
        let destination =
            VaultProfile::new("Destination", &destination_root).expect("destination profile");
        let source_id = source.id.clone();
        let destination_id = destination.id.clone();
        let mut catalog = VaultCatalog::default();
        catalog.add(source).expect("register source profile");
        catalog
            .add(destination)
            .expect("register destination profile");
        Self {
            base,
            source_root,
            destination_root,
            data_root,
            source_id,
            destination_id,
            catalog,
        }
    }

    fn pair(&self, source: &str, destination: &str, overwrite: bool) -> MovePair {
        MovePair::new(
            VaultFileRef::new(self.source_id.clone(), relative(source)),
            VaultFileRef::new(self.destination_id.clone(), relative(destination)),
            overwrite,
        )
    }

    fn plan(&self, pairs: Vec<MovePair>) -> MovePlan {
        MovePlan::build(pairs, &self.catalog, |_| false).expect("build confirmed move plan")
    }

    fn write_source(&self, path: &str, bytes: &[u8]) {
        write_file(&self.source_root, path, bytes);
    }

    fn write_destination(&self, path: &str, bytes: &[u8]) {
        write_file(&self.destination_root, path, bytes);
    }

    fn operation_root(&self, operation_id: &MoveOperationId) -> PathBuf {
        self.data_root.join("vault-ops").join(operation_id.as_str())
    }

    fn journal_state(&self, operation_id: &MoveOperationId) -> MoveJournalState {
        JournalStore::at_data_root(&self.data_root)
            .load(operation_id)
            .expect("load durable journal")
            .state
    }

    fn recovery(&self, retention_days: u64) -> MoveRecoveryService<'_> {
        MoveRecoveryService::at_data_root(
            &self.catalog,
            &self.data_root,
            MoveRecoveryConfig { retention_days },
        )
    }

    fn interrupt(&self, move_failure: MoveFailurePoint, overwrite: bool) -> MoveOperationId {
        self.write_source("note.md", b"source bytes");
        if overwrite {
            self.write_destination("moved.md", b"old destination");
        }
        let plan = self.plan(vec![self.pair("note.md", "moved.md", overwrite)]);
        let operation_id = MoveOperationId::from_plan(&plan);
        inject_move_interruption(move_failure);
        assert!(MoveExecutor::at_data_root(&self.catalog, &self.data_root)
            .execute(plan)
            .is_err());
        assert_eq!(
            self.journal_state(&operation_id),
            MoveJournalState::RollingBack
        );
        operation_id
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.base);
    }
}

fn relative(path: &str) -> VaultRelativePath {
    VaultRelativePath::parse(Path::new(path)).expect("valid Vault-relative test path")
}

fn write_file(root: &Path, path: &str, bytes: &[u8]) {
    let target = root.join(path);
    if let Some(parent) = target.parent() {
        fs::create_dir_all(parent).expect("create fixture parent");
    }
    fs::write(target, bytes).expect("write fixture file");
}

fn age_journal(path: &Path, days: u64) {
    let modified = SystemTime::now()
        .checked_sub(Duration::from_secs(days * 24 * 60 * 60))
        .expect("valid old timestamp");
    let journal = fs::File::options()
        .write(true)
        .open(path.join("journal.json"))
        .expect("open journal for timestamp update");
    journal
        .set_times(fs::FileTimes::new().set_modified(modified))
        .expect("age journal");
}

#[test]
fn vault_move_recovery_each_failed_step_is_durable_and_resumable() {
    let _env_guard = crate::test_support::env_lock()
        .lock()
        .expect("environment lock");
    for (recovery_failure, move_failure, overwrite) in [
        (
            RecoveryFailurePoint::RestoreSource,
            MoveFailurePoint::SourceDirectorySync,
            false,
        ),
        (
            RecoveryFailurePoint::RestoreDestination,
            MoveFailurePoint::DestinationDirectorySync,
            true,
        ),
        (
            RecoveryFailurePoint::RemoveCreatedDestination,
            MoveFailurePoint::DestinationDirectorySync,
            false,
        ),
        (
            RecoveryFailurePoint::CleanStagedFile,
            MoveFailurePoint::StageSync,
            false,
        ),
    ] {
        // Given
        let fixture = Fixture::new(recovery_failure.label());
        let operation_id = fixture.interrupt(move_failure, overwrite);
        inject_recovery_failure(recovery_failure);

        // When
        let first = fixture.recovery(7).recover_startup();

        // Then
        let first = first.expect("recovery scan completes with blocked report");
        assert!(first.blocked());
        assert_eq!(
            fixture.journal_state(&operation_id),
            MoveJournalState::RecoveryRequired,
            "{} failure was not durable",
            recovery_failure.label()
        );

        // Given a simulated restart / When recovery resumes
        drop(first);
        let second = fixture
            .recovery(7)
            .recover_startup()
            .expect("recovery resumes after restart");

        // Then the exact pre-operation state is restored
        assert!(!second.blocked());
        assert_eq!(
            fixture.journal_state(&operation_id),
            MoveJournalState::RolledBack
        );
        assert_eq!(
            fs::read(fixture.source_root.join("note.md")).expect("read restored source"),
            b"source bytes"
        );
        if overwrite {
            assert_eq!(
                fs::read(fixture.destination_root.join("moved.md"))
                    .expect("read restored destination"),
                b"old destination"
            );
        } else {
            assert!(!fixture.destination_root.join("moved.md").exists());
        }
    }
}

#[test]
fn vault_move_recovery_restart_and_second_rerun_are_idempotent() {
    let _env_guard = crate::test_support::env_lock()
        .lock()
        .expect("environment lock");
    // Given
    let fixture = Fixture::new("restart-idempotent");
    let operation_id = fixture.interrupt(MoveFailurePoint::SourceDirectorySync, false);

    // When a fresh service recovers twice
    let first = fixture
        .recovery(7)
        .recover_startup()
        .expect("first startup recovery");
    drop(first);
    let second = fixture
        .recovery(7)
        .recover_startup()
        .expect("second startup recovery");

    // Then
    assert!(!second.blocked());
    assert_eq!(
        fixture.journal_state(&operation_id),
        MoveJournalState::RolledBack
    );
    assert_eq!(
        fs::read(fixture.source_root.join("note.md")).expect("read source after rerun"),
        b"source bytes"
    );
    assert!(!fixture.destination_root.join("moved.md").exists());
}

#[test]
fn vault_move_recovery_failed_immediate_rollback_blocks_until_startup_resume() {
    let _env_guard = crate::test_support::env_lock()
        .lock()
        .expect("environment lock");
    // Given
    let fixture = Fixture::new("immediate-rollback-failure");
    fixture.write_source("note.md", b"source bytes");
    let plan = fixture.plan(vec![fixture.pair("note.md", "moved.md", false)]);
    let operation_id = MoveOperationId::from_plan(&plan);
    inject_move_failure(MoveFailurePoint::SourceDirectorySync);
    inject_recovery_failure(RecoveryFailurePoint::RestoreSource);

    // When
    let result = MoveExecutor::at_data_root(&fixture.catalog, &fixture.data_root).execute(plan);

    // Then
    assert!(matches!(
        result,
        Err(crate::vault::move_execute::MoveExecuteError::RecoveryRequired { .. })
    ));
    assert_eq!(
        fixture.journal_state(&operation_id),
        MoveJournalState::RecoveryRequired
    );
    let resumed = fixture
        .recovery(7)
        .recover_startup()
        .expect("resume degraded immediate rollback");
    assert!(!resumed.blocked());
    assert_eq!(
        fixture.journal_state(&operation_id),
        MoveJournalState::RolledBack
    );
}

#[test]
fn vault_move_recovery_retention_deletes_only_old_known_terminal_sets() {
    let _env_guard = crate::test_support::env_lock()
        .lock()
        .expect("environment lock");
    // Given an old Complete set
    let fixture = Fixture::new("retention");
    fixture.write_source("complete.md", b"complete source");
    let complete_plan = fixture.plan(vec![fixture.pair("complete.md", "complete.md", false)]);
    let complete_id = MoveOperationId::from_plan(&complete_plan);
    MoveExecutor::at_data_root(&fixture.catalog, &fixture.data_root)
        .execute(complete_plan)
        .expect("complete move");

    // Given an old RolledBack set
    let rolled_back_id = fixture.interrupt(MoveFailurePoint::SourceDirectorySync, false);
    fixture
        .recovery(7)
        .recover_startup()
        .expect("produce rolled-back journal");

    // Given an old RecoveryRequired set
    fs::remove_file(fixture.source_root.join("note.md")).expect("prepare second interruption");
    let recovery_required_id = fixture.interrupt(MoveFailurePoint::SourceDirectorySync, false);
    inject_recovery_failure(RecoveryFailurePoint::RestoreSource);
    fixture
        .recovery(7)
        .recover_startup()
        .expect("persist recovery-required journal");

    for operation_id in [&complete_id, &rolled_back_id, &recovery_required_id] {
        age_journal(&fixture.operation_root(operation_id), 8);
    }
    let unknown = fixture
        .data_root
        .join("vault-ops")
        .join("move-00000000000000000000000000000000");
    fs::create_dir_all(&unknown).expect("create unknown operation");
    fs::write(unknown.join("journal.json"), br#"{"version":99}"#).expect("write unknown journal");
    age_journal(&unknown, 8);
    let malformed = fixture
        .data_root
        .join("vault-ops")
        .join("move-11111111111111111111111111111111");
    fs::create_dir_all(&malformed).expect("create malformed operation");
    fs::write(malformed.join("journal.json"), b"not-json").expect("write malformed journal");
    age_journal(&malformed, 8);
    inject_recovery_failure(RecoveryFailurePoint::RestoreSource);

    // When
    let report = fixture
        .recovery(7)
        .recover_startup()
        .expect("retention scan");

    // Then
    assert!(report.blocked());
    assert!(!fixture.operation_root(&complete_id).exists());
    assert!(!fixture.operation_root(&rolled_back_id).exists());
    assert!(fixture.operation_root(&recovery_required_id).exists());
    assert!(unknown.exists());
    assert!(malformed.exists());
}

#[test]
fn vault_move_recovery_retention_config_defaults_to_seven_days() {
    let _env_guard = crate::test_support::env_lock()
        .lock()
        .expect("environment lock");
    // Given / When
    let config = crate::config::AppConfig::default();

    // Then
    assert_eq!(config.vault_operation_retention_days, 7);
}

#[test]
fn vault_move_recovery_is_blocked_while_an_operation_holds_the_global_lock() {
    let _env_guard = crate::test_support::env_lock()
        .lock()
        .expect("environment lock");
    // Given
    let fixture = Fixture::new("lock-contention");
    let operation_id = fixture.interrupt(MoveFailurePoint::StageSync, false);
    let lock = VaultMutationLock::at_data_root(&fixture.data_root);
    let _guard = lock.try_acquire().expect("hold mutation lock");

    // When
    let result = fixture.recovery(7).recover_startup();

    // Then
    assert!(matches!(result, Err(VaultWriteError::VaultBusy)));
    assert_eq!(
        fixture.journal_state(&operation_id),
        MoveJournalState::RollingBack
    );
}

#[test]
fn vault_move_recovery_report_redacts_roots_and_contents() {
    let _env_guard = crate::test_support::env_lock()
        .lock()
        .expect("environment lock");
    // Given
    let fixture = Fixture::new("redacted-report");
    let operation_id = fixture.interrupt(MoveFailurePoint::SourceDirectorySync, false);

    // When
    let report = fixture
        .recovery(7)
        .recover_startup()
        .expect("recover for report");
    let lines = report.log_lines().join("\n");

    // Then
    assert!(report
        .records()
        .iter()
        .any(|record| record.status == RecoveryStatus::RolledBack));
    assert!(lines.contains(operation_id.as_str()));
    assert!(lines.contains("note.md"));
    assert!(!lines.contains(&fixture.source_root.display().to_string()));
    assert!(!lines.contains(&fixture.destination_root.display().to_string()));
    assert!(!lines.contains("source bytes"));
}
