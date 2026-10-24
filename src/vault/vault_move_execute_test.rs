// allow: SIZE_OK - T05B integration tests cover the complete durable transaction boundary.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::config::vault_profiles::{VaultCatalog, VaultProfile, VaultProfileId};
use crate::vault::move_execute::{
    inject_failure, take_test_events, MoveExecuteError, MoveExecutor, TestEvent, TestFailurePoint,
};
use crate::vault::move_journal::{JournalStore, MoveJournalState, MoveOperationId};
use crate::vault::move_plan::{MovePair, MovePlan, VaultFileRef};
use crate::vault::{
    VaultMutationLock, VaultProfileRegistration, VaultRelativePath, VaultWriteError,
    VaultWriteService,
};

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
            "tcui-vault-move-execute-{label}-{}-{}",
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
            self.file_ref(self.source_id.clone(), source),
            self.file_ref(self.destination_id.clone(), destination),
            overwrite,
        )
    }

    fn file_ref(&self, profile_id: VaultProfileId, path: &str) -> VaultFileRef {
        VaultFileRef::new(profile_id, relative(path))
    }

    fn write_source(&self, path: &str, bytes: &[u8]) {
        write_file(&self.source_root, path, bytes);
    }

    fn write_destination(&self, path: &str, bytes: &[u8]) {
        write_file(&self.destination_root, path, bytes);
    }

    fn plan(&self, pairs: Vec<MovePair>) -> MovePlan {
        MovePlan::build(pairs, &self.catalog, |_| false).expect("build confirmed move plan")
    }

    fn executor(&self) -> MoveExecutor<'_> {
        MoveExecutor::at_data_root(&self.catalog, &self.data_root)
    }

    fn journal(&self, operation_id: &MoveOperationId) -> crate::vault::move_journal::MoveJournalV1 {
        JournalStore::at_data_root(&self.data_root)
            .load(operation_id)
            .expect("load durable journal")
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

#[test]
fn move_execution_dual_handle_contention_returns_busy_without_journal() {
    let _env_guard = crate::test_support::env_lock()
        .lock()
        .expect("environment lock");
    // Given
    let fixture = Fixture::new("dual-handle-contention");
    fixture.write_source("note.md", b"source");
    let plan = fixture.plan(vec![fixture.pair("note.md", "moved.md", false)]);
    let lock = VaultMutationLock::at_data_root(&fixture.data_root);
    let _guard = lock.try_acquire().expect("hold mutation lock");

    // When
    let result = fixture.executor().execute(plan);

    // Then
    assert!(matches!(
        result,
        Err(MoveExecuteError::Write(VaultWriteError::VaultBusy))
    ));
    assert!(fixture.source_root.join("note.md").exists());
    assert!(!fixture.data_root.join("vault-ops").exists());
}

#[test]
fn move_execution_cross_process_contention_returns_busy_without_journal() {
    // Given
    let fixture = Fixture::new("cross-process-contention");
    fs::create_dir_all(&fixture.data_root).expect("create data root");
    let lock = VaultMutationLock::at_data_root(&fixture.data_root);
    let _guard = lock.try_acquire().expect("hold parent mutation lock");

    // When
    let status = Command::new(std::env::current_exe().expect("test executable"))
        .arg("--exact")
        .arg("vault::vault_move_execute::move_execution_lock_child_probe")
        .arg("--nocapture")
        .arg("--test-threads=1")
        .env("TCUI_MOVE_LOCK_CHILD_ROOT", &fixture.data_root)
        .status()
        .expect("run lock child process");

    // Then
    assert!(status.success());
    assert!(!fixture.data_root.join("vault-ops").exists());
}

#[test]
fn move_execution_lock_child_probe() {
    let Some(root) = std::env::var_os("TCUI_MOVE_LOCK_CHILD_ROOT") else {
        return;
    };
    let result = VaultMutationLock::at_data_root(Path::new(&root)).try_acquire();
    assert!(matches!(result, Err(VaultWriteError::VaultBusy)));
}

#[test]
fn move_execution_failures_rollback_before_returning() {
    let _env_guard = crate::test_support::env_lock()
        .lock()
        .expect("environment lock");
    for point in [
        TestFailurePoint::JournalTransition,
        TestFailurePoint::Backup,
        TestFailurePoint::StageWrite,
        TestFailurePoint::StageSync,
        TestFailurePoint::DestinationRename,
        TestFailurePoint::DestinationDirectorySync,
        TestFailurePoint::SourceRemove,
    ] {
        // Given
        let fixture = Fixture::new(point.label());
        fixture.write_source("note.md", b"source bytes");
        fixture.write_destination("moved.md", b"old destination");
        let plan = fixture.plan(vec![fixture.pair("note.md", "moved.md", true)]);
        let operation_id = MoveOperationId::from_plan(&plan);
        inject_failure(point);

        // When
        let result = fixture.executor().execute(plan);

        // Then
        assert!(
            matches!(&result, Err(MoveExecuteError::InjectedFailure { .. })),
            "failure point {} returned {result:?}",
            point.label()
        );
        assert_eq!(
            fixture.journal(&operation_id).state,
            MoveJournalState::RolledBack,
            "failure point {} did not finish rollback",
            point.label()
        );
        assert!(
            fixture.source_root.join("note.md").exists(),
            "failure point {} removed the source",
            point.label()
        );
    }
}

#[test]
fn move_execution_source_sync_failure_restores_pre_operation_state() {
    let _env_guard = crate::test_support::env_lock()
        .lock()
        .expect("environment lock");
    // Given
    let fixture = Fixture::new("source-sync");
    fixture.write_source("note.md", b"source bytes");
    let plan = fixture.plan(vec![fixture.pair("note.md", "moved.md", false)]);
    let operation_id = MoveOperationId::from_plan(&plan);
    inject_failure(TestFailurePoint::SourceDirectorySync);

    // When
    let result = fixture.executor().execute(plan);

    // Then
    assert!(matches!(
        result,
        Err(MoveExecuteError::InjectedFailure { .. })
    ));
    assert_eq!(
        fixture.journal(&operation_id).state,
        MoveJournalState::RolledBack
    );
    assert_eq!(
        fs::read(fixture.source_root.join("note.md")).expect("read restored source"),
        b"source bytes"
    );
    assert!(!fixture.destination_root.join("moved.md").exists());
}

#[test]
fn move_execution_same_profile_move_and_rollback_preserve_transaction_guarantees() {
    let _env_guard = crate::test_support::env_lock()
        .lock()
        .expect("environment lock");
    for failure in [None, Some(TestFailurePoint::SourceDirectorySync)] {
        // Given
        let fixture = Fixture::new(match failure {
            None => "same-profile-happy",
            Some(_) => "same-profile-rollback",
        });
        fixture.write_source("notes/readme.md", b"source bytes");
        let pair = MovePair::new(
            fixture.file_ref(fixture.source_id.clone(), "notes/readme.md"),
            fixture.file_ref(fixture.source_id.clone(), "archive/readme.md"),
            false,
        );
        let plan = fixture.plan(vec![pair]);
        if let Some(point) = failure {
            inject_failure(point);
        }

        // When
        let result = fixture.executor().execute(plan);

        // Then
        if failure.is_some() {
            assert!(matches!(
                result,
                Err(MoveExecuteError::InjectedFailure { .. })
            ));
            assert_eq!(
                fs::read(fixture.source_root.join("notes/readme.md")).expect("restored source"),
                b"source bytes"
            );
            assert!(!fixture.source_root.join("archive/readme.md").exists());
        } else {
            result.expect("same-profile move");
            assert!(!fixture.source_root.join("notes/readme.md").exists());
            assert_eq!(
                fs::read(fixture.source_root.join("archive/readme.md")).expect("moved destination"),
                b"source bytes"
            );
        }
    }
}

#[test]
fn move_execution_journal_create_failure_never_mutates_vault() {
    let _env_guard = crate::test_support::env_lock()
        .lock()
        .expect("environment lock");
    // Given
    let fixture = Fixture::new("journal-create");
    fixture.write_source("note.md", b"source bytes");
    let plan = fixture.plan(vec![fixture.pair("note.md", "moved.md", false)]);
    inject_failure(TestFailurePoint::JournalCreate);

    // When
    let result = fixture.executor().execute(plan);

    // Then
    assert!(matches!(
        result,
        Err(MoveExecuteError::InjectedFailure { .. })
    ));
    assert!(fixture.source_root.join("note.md").exists());
    assert!(!fixture.destination_root.join("moved.md").exists());
}

#[test]
fn move_execution_precommit_revalidation_failure_never_starts_journal() {
    let _env_guard = crate::test_support::env_lock()
        .lock()
        .expect("environment lock");
    // Given
    let fixture = Fixture::new("precommit-revalidation");
    fixture.write_source("note.md", b"source bytes");
    let plan = fixture.plan(vec![fixture.pair("note.md", "moved.md", false)]);
    fixture.write_source("note.md", b"changed bytes");

    // When
    let result = fixture.executor().execute(plan);

    // Then
    assert!(matches!(
        result,
        Err(MoveExecuteError::Plan(
            crate::vault::move_plan::MovePlanError::SourceChanged { .. }
        ))
    ));
    assert_eq!(
        fs::read(fixture.source_root.join("note.md")).expect("read changed source"),
        b"changed bytes"
    );
    assert!(!fixture.destination_root.join("moved.md").exists());
    assert!(!fixture.data_root.join("vault-ops").exists());
}

#[test]
fn move_execution_state_transitions_are_durable_after_each_write() {
    let _env_guard = crate::test_support::env_lock()
        .lock()
        .expect("environment lock");
    // Given
    let fixture = Fixture::new("durable-transitions");
    fixture.write_source("note.md", b"source bytes");
    let plan = fixture.plan(vec![fixture.pair("note.md", "moved.md", false)]);
    let store = JournalStore::at_data_root(&fixture.data_root);
    let mut journal = crate::vault::move_journal::MoveJournalV1::planned(&plan);
    store.create(&journal).expect("create planned journal");

    // When / Then
    for state in [
        MoveJournalState::Planned,
        MoveJournalState::Staging,
        MoveJournalState::DestinationsCommitted,
        MoveJournalState::SourcesRemoving,
        MoveJournalState::Complete,
    ] {
        if state != MoveJournalState::Planned {
            store
                .transition(&mut journal, state)
                .expect("persist legal transition");
        }
        assert_eq!(
            store
                .load(&journal.operation_id)
                .expect("reload transition")
                .state,
            state
        );
    }
}

#[test]
fn move_execution_happy_path_orders_durability_before_source_removal() {
    let _env_guard = crate::test_support::env_lock()
        .lock()
        .expect("environment lock");
    // Given
    let fixture = Fixture::new("happy-path");
    fixture.write_source("first.md", b"first bytes");
    fixture.write_source("nested/second.md", b"second bytes");
    fixture.write_destination("second.md", b"old second");
    let plan = fixture.plan(vec![
        fixture.pair("first.md", "first.md", false),
        fixture.pair("nested/second.md", "second.md", true),
    ]);
    let operation_id = MoveOperationId::from_plan(&plan);

    // When
    let outcome = fixture.executor().execute(plan).expect("execute move");

    // Then
    assert_eq!(outcome.operation_id, operation_id);
    assert_eq!(
        fixture.journal(&operation_id).state,
        MoveJournalState::Complete
    );
    assert!(!fixture.source_root.join("first.md").exists());
    assert!(!fixture.source_root.join("nested/second.md").exists());
    assert_eq!(
        fs::read(fixture.destination_root.join("first.md")).expect("read first destination"),
        b"first bytes"
    );
    assert_eq!(
        fs::read(fixture.destination_root.join("second.md")).expect("read second destination"),
        b"second bytes"
    );
    let events = take_test_events();
    let last_destination_sync = events
        .iter()
        .rposition(|event| *event == TestEvent::DestinationDirectorySynced)
        .expect("destination sync event");
    let first_source_remove = events
        .iter()
        .position(|event| *event == TestEvent::SourceRemoveStarted)
        .expect("source removal event");
    assert!(last_destination_sync < first_source_remove);
}

#[test]
fn move_execution_unknown_journal_version_blocks_and_retains_all_data() {
    let _env_guard = crate::test_support::env_lock()
        .lock()
        .expect("environment lock");
    // Given
    let fixture = Fixture::new("unknown-version");
    fixture.write_source("note.md", b"source bytes");
    let plan = fixture.plan(vec![fixture.pair("note.md", "moved.md", false)]);
    let unknown = fixture
        .data_root
        .join("vault-ops")
        .join("move-00000000000000000000000000000000");
    fs::create_dir_all(&unknown).expect("create unknown operation");
    let unknown_bytes = br#"{"version":99,"retained":"sentinel"}"#;
    fs::write(unknown.join("journal.json"), unknown_bytes).expect("write unknown journal");

    // When
    let result = fixture.executor().execute(plan);

    // Then
    assert!(matches!(
        result,
        Err(MoveExecuteError::UnknownJournalVersion { version: 99 })
    ));
    assert_eq!(
        fs::read(unknown.join("journal.json")).expect("read retained unknown journal"),
        unknown_bytes
    );
    assert!(fixture.source_root.join("note.md").exists());
    assert!(!fixture.destination_root.join("moved.md").exists());
}

#[test]
fn move_execution_unknown_journal_version_blocks_standalone_write() {
    let _env_guard = crate::test_support::env_lock()
        .lock()
        .expect("environment lock");
    // Given
    let fixture = Fixture::new("unknown-version-write");
    fixture.write_source("note.md", b"source bytes");
    let unknown = fixture
        .data_root
        .join("vault-ops")
        .join("move-00000000000000000000000000000000");
    fs::create_dir_all(&unknown).expect("create unknown operation");
    fs::write(unknown.join("journal.json"), br#"{"version":99}"#).expect("write unknown journal");
    let service = VaultWriteService::at_data_root(
        [VaultProfileRegistration::new(
            fixture.source_id.clone(),
            fixture.source_root.clone(),
        )],
        &fixture.data_root,
    )
    .expect("open write service");

    // When
    let result = service.write(&fixture.source_id, &relative("note.md"), b"replacement");

    // Then
    assert!(matches!(
        result,
        Err(VaultWriteError::UnsupportedMoveJournalVersion { version: 99 })
    ));
    assert_eq!(
        fs::read(fixture.source_root.join("note.md")).expect("read retained source"),
        b"source bytes"
    );
    assert!(unknown.exists());
}

#[cfg(unix)]
#[test]
fn move_execution_journal_storage_uses_private_unix_modes() {
    use std::os::unix::fs::PermissionsExt as _;

    let _env_guard = crate::test_support::env_lock()
        .lock()
        .expect("environment lock");
    // Given
    let fixture = Fixture::new("permissions");
    fixture.write_source("note.md", b"source bytes");
    let plan = fixture.plan(vec![fixture.pair("note.md", "moved.md", false)]);
    let operation_id = MoveOperationId::from_plan(&plan);

    // When
    fixture.executor().execute(plan).expect("execute move");

    // Then
    let operation_root = fixture
        .data_root
        .join("vault-ops")
        .join(operation_id.as_str());
    let directory_mode = fs::metadata(&operation_root)
        .expect("operation metadata")
        .permissions()
        .mode()
        & 0o777;
    let journal_mode = fs::metadata(operation_root.join("journal.json"))
        .expect("journal metadata")
        .permissions()
        .mode()
        & 0o777;
    assert_eq!(directory_mode, 0o700);
    assert_eq!(journal_mode, 0o600);
}
