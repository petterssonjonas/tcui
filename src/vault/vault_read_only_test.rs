use std::collections::HashMap;
use std::path::{Path, PathBuf};

use serde_json::{json, Map, Value};

use super::read_tools::{
    execute_read_only, parse_read_only_operation, VaultReadOnlyData, VaultReadOnlyLimits,
    VaultReadOnlyOperation, VaultReadOnlyStatus,
};
use crate::config::vault_profiles::{VaultProfile, VaultProfileId};
use crate::vault::{VaultLimitKind, VaultSearchLimits, VaultWalkLimits};

struct ReadFixture {
    root: PathBuf,
    profiles: HashMap<VaultProfileId, VaultProfile>,
    id: VaultProfileId,
}

impl ReadFixture {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!(
            "tcui-vault-read-tools-{}-{}",
            std::process::id(),
            VaultProfileId::new()
        ));
        std::fs::create_dir_all(&root).expect("create read tool directory");
        let profile = VaultProfile::new("Work Notes", &root).expect("valid read tool profile");
        let id = profile.id.clone();
        let profiles = HashMap::from([(id.clone(), profile)]);
        Self { root, profiles, id }
    }

    fn write(&self, relative: &str, content: &str) {
        let path = self.root.join(relative);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).expect("create fixture parent");
        }
        std::fs::write(path, content).expect("write read tool fixture");
    }
}

impl Drop for ReadFixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

fn arguments(value: Value) -> Map<String, Value> {
    value.as_object().cloned().expect("object arguments")
}

#[test]
fn vault_target_resolution_parses_profile_id_and_relative_path_into_typed_read() {
    let fixture = ReadFixture::new();

    let operation = parse_read_only_operation(
        fixture.id.clone(),
        "read_note",
        &arguments(json!({"path": "projects/roadmap.md"})),
    )
    .expect("typed read operation");

    assert!(matches!(
        operation,
        VaultReadOnlyOperation::Read { profile_id, path }
            if profile_id == fixture.id && path.as_path() == Path::new("projects/roadmap.md")
    ));
}

#[test]
fn vault_target_resolution_rejects_non_relative_tool_path_before_io() {
    let fixture = ReadFixture::new();

    let result = parse_read_only_operation(
        fixture.id.clone(),
        "read_note",
        &arguments(json!({"path": "../outside.md"})),
    );

    assert!(result.is_err());
}

#[test]
fn vault_target_resolution_bounded_read_reports_partial_file_limit() {
    let _env_guard = crate::test_support::env_lock()
        .lock()
        .expect("environment lock");
    let fixture = ReadFixture::new();
    fixture.write("large.md", "abcdef");
    let operation = parse_read_only_operation(
        fixture.id.clone(),
        "read_note",
        &arguments(json!({"path": "large.md"})),
    )
    .expect("typed read operation");
    let limits = VaultReadOnlyLimits {
        max_read_bytes: 5,
        ..VaultReadOnlyLimits::default()
    };

    let result = execute_read_only(&fixture.profiles, &operation, limits).expect("bounded read");

    assert_eq!(result.status, VaultReadOnlyStatus::Partial);
    assert!(result
        .limits
        .iter()
        .any(|limit| limit.kind == VaultLimitKind::FileBytes));
    assert!(matches!(
        result.data,
        VaultReadOnlyData::Read { content, .. } if content == "abcde"
    ));
}

#[test]
fn vault_target_resolution_default_read_bound_matches_t04a_file_limit() {
    assert_eq!(
        VaultReadOnlyLimits::default().max_read_bytes,
        VaultSearchLimits::default().max_file_bytes
    );
}

#[test]
fn vault_target_resolution_bounded_search_reports_partial_match_limit() {
    let _env_guard = crate::test_support::env_lock()
        .lock()
        .expect("environment lock");
    let fixture = ReadFixture::new();
    fixture.write("notes.md", "needle needle");
    let operation = parse_read_only_operation(
        fixture.id.clone(),
        "search_notes",
        &arguments(json!({"query": "needle"})),
    )
    .expect("typed search operation");
    let limits = VaultReadOnlyLimits {
        walk: VaultWalkLimits::default(),
        search: VaultSearchLimits {
            max_matches: 1,
            ..VaultSearchLimits::default()
        },
        max_read_bytes: 1024 * 1024,
    };

    let result = execute_read_only(&fixture.profiles, &operation, limits).expect("bounded search");

    assert_eq!(result.status, VaultReadOnlyStatus::Partial);
    assert!(result
        .limits
        .iter()
        .any(|limit| limit.kind == VaultLimitKind::Matches));
    assert!(matches!(
        result.data,
        VaultReadOnlyData::Search { matches, .. } if matches.len() == 1
    ));
}

#[test]
fn vault_target_resolution_list_returns_relative_paths_without_profile_root() {
    let _env_guard = crate::test_support::env_lock()
        .lock()
        .expect("environment lock");
    let fixture = ReadFixture::new();
    fixture.write("projects/roadmap.md", "roadmap");
    let operation = parse_read_only_operation(
        fixture.id.clone(),
        "list_notes",
        &arguments(json!({"path": "projects"})),
    )
    .expect("typed list operation");

    let result = execute_read_only(
        &fixture.profiles,
        &operation,
        VaultReadOnlyLimits::default(),
    )
    .expect("confined list");

    assert_eq!(result.display_name, "Work Notes");
    assert!(matches!(
        result.data,
        VaultReadOnlyData::List(entries) if entries.iter().any(|entry| entry.path == Path::new("projects/roadmap.md"))
    ));
}
