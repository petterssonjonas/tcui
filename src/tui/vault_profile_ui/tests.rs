#![cfg(test)]

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use super::{
    launcher_entry, LauncherEntry, VaultProfileManagerState, VaultProfilePhase,
    VaultProfileUiCommand, VaultProfileUiError,
};
use crate::config::vault_profiles::{
    VaultCatalog, VaultProfile, VaultProfileError, VaultProfileId,
};
use crate::ui::app_tabs::{AppTabs, AppView};

#[path = "flow_tests.rs"]
mod flow_tests;

struct TestDir(PathBuf);

impl TestDir {
    fn new(label: &str) -> Self {
        let path = std::env::temp_dir().join(format!(
            "tcui-vault-profile-ui-{label}-{}-{}",
            std::process::id(),
            VaultProfileId::new()
        ));
        std::fs::create_dir_all(&path).expect("create test directory");
        Self(path)
    }

    fn child(&self, name: &str) -> PathBuf {
        let path = self.0.join(name);
        std::fs::create_dir_all(&path).expect("create child directory");
        path
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TestDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn add_profile(catalog: &mut VaultCatalog, name: &str, root: &Path) -> VaultProfileId {
    catalog
        .add(VaultProfile::new(name, root).expect("valid profile"))
        .expect("add profile")
}

fn persisted(command: VaultProfileUiCommand) -> (VaultCatalog, Option<VaultProfileId>) {
    match command {
        VaultProfileUiCommand::Persist {
            catalog,
            activate_profile_id,
        } => (catalog, activate_profile_id),
        other => panic!("expected persistence command, got {other:?}"),
    }
}

#[test]
fn add_cancel_error_retry_flow_preserves_input_and_launcher() {
    // Given
    let root = TestDir::new("add-retry");
    let vault = root.child("Personal Notes");
    let mut tabs = AppTabs::default();
    let launcher_id = tabs.add_launcher();
    let LauncherEntry::Add(mut manager) =
        launcher_entry(&VaultCatalog::default(), launcher_id, root.path()).expect("add flow")
    else {
        panic!("zero profiles must open add flow");
    };

    // When / Then: cancellation has no side effects.
    assert_eq!(manager.cancel(), VaultProfileUiCommand::Dismiss);
    assert_eq!(tabs.active_id(), launcher_id);
    assert_eq!(
        tabs.active_tab().map(|tab| tab.view()),
        Some(&AppView::Launcher)
    );

    // When: selection advances to a prefilled name, invalid input is retried.
    manager.choose_directory(&vault).expect("choose directory");
    assert_eq!(manager.phase(), VaultProfilePhase::Name);
    assert_eq!(manager.input(), "Personal Notes");
    manager.set_input("   ");
    assert_eq!(
        manager.submit_name(&HashSet::new()),
        VaultProfileUiCommand::None
    );

    // Then
    assert_eq!(manager.input(), "   ");
    assert!(manager.error().is_some());
    manager.set_input("Personal");
    let (catalog, activate) = persisted(manager.submit_name(&HashSet::new()));
    assert_eq!(catalog.profiles.len(), 1);
    assert_eq!(catalog.default_id, activate);
}

#[test]
fn duplicate_name_and_canonical_path_are_rejected_without_losing_draft() {
    // Given
    let root = TestDir::new("duplicates");
    let first = root.child("first");
    let second = root.child("second");
    let mut catalog = VaultCatalog::default();
    add_profile(&mut catalog, "Notes", &first);
    let mut manager =
        VaultProfileManagerState::for_settings(catalog, root.path()).expect("settings manager");

    // When / Then: duplicate normalized name.
    manager.choose_directory(&second).expect("choose second");
    manager.set_input("ＮＯＴＥＳ");
    assert_eq!(
        manager.submit_name(&HashSet::new()),
        VaultProfileUiCommand::None
    );
    assert!(matches!(
        manager.last_error(),
        Some(VaultProfileUiError::Profile(_))
    ));
    assert_eq!(manager.input(), "ＮＯＴＥＳ");

    // When / Then: duplicate canonical path.
    manager
        .choose_directory(&first.join("..").join("first"))
        .expect("choose alias");
    manager.set_input("Other");
    assert_eq!(
        manager.submit_name(&HashSet::new()),
        VaultProfileUiCommand::None
    );
    assert!(matches!(
        manager.last_error(),
        Some(VaultProfileUiError::Profile(_))
    ));
}

#[test]
fn first_profile_becomes_default_and_later_profiles_do_not_replace_it() {
    // Given
    let root = TestDir::new("first-default");
    let first = root.child("first");
    let second = root.child("second");
    let mut manager = VaultProfileManagerState::for_settings(VaultCatalog::default(), root.path())
        .expect("settings manager");

    // When
    manager.choose_directory(&first).expect("choose first");
    manager.set_input("First");
    let (catalog, _) = persisted(manager.submit_name(&HashSet::new()));
    let first_id = catalog.default_id.clone().expect("first default");
    manager.commit_catalog(catalog);
    manager.begin_add(root.path()).expect("begin second add");
    manager.choose_directory(&second).expect("choose second");
    manager.set_input("Second");
    let (catalog, _) = persisted(manager.submit_name(&HashSet::new()));

    // Then
    assert_eq!(catalog.default_id, Some(first_id));
    assert_eq!(catalog.profiles.len(), 2);
}

#[test]
fn rename_default_and_remove_preserve_identity_and_never_touch_files() {
    // Given
    let root = TestDir::new("crud");
    let first = root.child("first");
    let second = root.child("second");
    let replacement = root.child("replacement");
    let marker = second.join("keep.md");
    std::fs::write(&marker, "keep").expect("write marker");
    let mut catalog = VaultCatalog::default();
    let internal = VaultProfile::internal();
    let internal_id = catalog.add(internal.clone()).expect("add internal profile");
    let first_id = add_profile(&mut catalog, "First", &first);
    let second_id = add_profile(&mut catalog, "Second", &second);
    assert_eq!(catalog.default_id, Some(internal_id.clone()));
    let mut manager =
        VaultProfileManagerState::for_settings(catalog, root.path()).expect("settings manager");

    // When
    let (catalog, _) = persisted(manager.rename(&second_id, "Renamed", &HashSet::new()));
    manager.commit_catalog(catalog);
    let (catalog, _) = persisted(manager.repoint(&second_id, &replacement, &HashSet::new()));
    assert_eq!(
        catalog
            .get(&second_id)
            .map(|profile| profile.root.as_path()),
        Some(
            std::fs::canonicalize(&replacement)
                .expect("canonical replacement")
                .as_path()
        )
    );
    manager.commit_catalog(catalog);
    let (catalog, _) = persisted(manager.set_default(&second_id));
    assert_eq!(catalog.default_id, Some(second_id.clone()));
    manager.commit_catalog(catalog);
    let (catalog, _) = persisted(manager.remove(&first_id, &HashSet::new()));

    // Then
    assert_eq!(catalog.default_id, Some(second_id.clone()));
    assert_eq!(
        catalog
            .get(&second_id)
            .map(|profile| profile.display_name.as_str()),
        Some("Renamed")
    );
    assert!(catalog.get(&first_id).is_none());
    assert_eq!(catalog.get(&internal_id), Some(&internal));
    assert!(marker.exists());
}

#[test]
fn bound_profile_cannot_be_removed_or_repointed() {
    // Given
    let root = TestDir::new("bound");
    let first = root.child("first");
    let replacement = root.child("replacement");
    let mut catalog = VaultCatalog::default();
    let id = add_profile(&mut catalog, "First", &first);
    let mut manager =
        VaultProfileManagerState::for_settings(catalog, root.path()).expect("settings manager");
    let bound = HashSet::from([id.clone()]);

    // When / Then
    assert_eq!(manager.remove(&id, &bound), VaultProfileUiCommand::None);
    assert!(matches!(
        manager.last_error(),
        Some(VaultProfileUiError::ProfileIsOpen { .. })
    ));
    assert_eq!(
        manager.repoint(&id, &replacement, &bound),
        VaultProfileUiCommand::None
    );
    assert!(matches!(
        manager.last_error(),
        Some(VaultProfileUiError::ProfileIsOpen { .. })
    ));
    assert_eq!(
        manager
            .catalog()
            .get(&id)
            .map(|profile| profile.root.as_path()),
        Some(first.as_path())
    );
}

#[test]
fn builtin_profiles_with_reserved_or_reused_ids_cannot_be_removed_or_relocated() {
    let _guard = crate::test_support::env_lock()
        .lock()
        .expect("environment lock");
    struct DataHome(Option<std::ffi::OsString>);
    impl Drop for DataHome {
        fn drop(&mut self) {
            match &self.0 {
                Some(previous) => std::env::set_var("XDG_DATA_HOME", previous),
                None => std::env::remove_var("XDG_DATA_HOME"),
            }
        }
    }

    let root = TestDir::new("builtin-guards");
    let _data_home = DataHome(std::env::var_os("XDG_DATA_HOME"));
    std::env::set_var("XDG_DATA_HOME", root.path());
    let replacement = root.child("replacement");
    let internal = VaultProfile::internal();
    std::fs::create_dir_all(&internal.root).expect("create internal root");
    let source = internal.root.join("keep.md");
    std::fs::write(&source, b"original internal content").expect("write source");
    let reused_id = VaultProfileId::new();
    assert_ne!(reused_id, VaultProfileId::internal());

    for id in [VaultProfileId::internal(), reused_id] {
        let mut profile = internal.clone();
        profile.id = id.clone();
        let mut catalog = VaultCatalog::default();
        catalog.add(profile).expect("add internal profile");
        let mut manager =
            VaultProfileManagerState::for_settings(catalog, root.path()).expect("settings manager");

        let (catalog, _) = persisted(manager.rename(&id, "Private Notes", &HashSet::new()));
        let renamed = catalog.get(&id).expect("renamed internal profile");
        assert_eq!(renamed.display_name, "Private Notes");
        assert_eq!(renamed.root, internal.root);
        manager.commit_catalog(catalog.clone());

        assert_eq!(
            manager.remove(&id, &HashSet::new()),
            VaultProfileUiCommand::None
        );
        assert!(matches!(
            manager.last_error(),
            Some(VaultProfileUiError::Profile(
                VaultProfileError::BuiltinProfile
            ))
        ));
        assert_eq!(manager.catalog(), &catalog);
        assert_eq!(
            std::fs::read(&source).expect("source survives removal attempt"),
            b"original internal content"
        );
        assert_eq!(
            manager.repoint(&id, &replacement, &HashSet::new()),
            VaultProfileUiCommand::None
        );
        assert!(matches!(
            manager.last_error(),
            Some(VaultProfileUiError::Profile(
                VaultProfileError::BuiltinProfile
            ))
        ));
        assert_eq!(manager.catalog(), &catalog);
        assert_eq!(
            std::fs::read(&source).expect("source survives relocation attempt"),
            b"original internal content"
        );
        assert!(!replacement.join("keep.md").exists());
    }
}
