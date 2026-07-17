use std::collections::HashMap;
use std::path::{Path, PathBuf};

use super::target::{VaultMention, VaultTargetError, VaultTargetResolver};
use crate::config::vault_profiles::{VaultProfile, VaultProfileId};

struct TestDirectory(PathBuf);

impl TestDirectory {
    fn new(label: &str) -> Self {
        let path = std::env::temp_dir().join(format!(
            "tcui-vault-target-{label}-{}-{}",
            std::process::id(),
            VaultProfileId::new()
        ));
        std::fs::create_dir_all(&path).expect("create target test directory");
        Self(path)
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TestDirectory {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

struct TargetFixture {
    _roots: Vec<TestDirectory>,
    profiles: HashMap<VaultProfileId, VaultProfile>,
    default_id: Option<VaultProfileId>,
}

impl TargetFixture {
    fn new(names: &[&str], default_index: Option<usize>) -> Self {
        let mut roots = Vec::new();
        let mut profiles = HashMap::new();
        let mut ids = Vec::new();
        for (index, name) in names.iter().enumerate() {
            let root = TestDirectory::new(&format!("{index}"));
            let profile = VaultProfile::new(name, root.path()).expect("valid target profile");
            ids.push(profile.id.clone());
            profiles.insert(profile.id.clone(), profile);
            roots.push(root);
        }
        Self {
            _roots: roots,
            profiles,
            default_id: default_index.and_then(|index| ids.get(index).cloned()),
        }
    }

    fn resolver(&self) -> VaultTargetResolver<'_> {
        VaultTargetResolver::new(&self.profiles, self.default_id.as_ref())
    }

    fn id(&self, name: &str) -> VaultProfileId {
        self.profiles
            .values()
            .find(|profile| profile.display_name == name)
            .map(|profile| profile.id.clone())
            .expect("fixture profile")
    }
}

#[test]
fn vault_target_resolution_normalizes_explicit_quoted_names() {
    let fixture = TargetFixture::new(&["Café"], Some(0));

    let target = fixture
        .resolver()
        .resolve("@vault \"Cafe\u{301}\" summarize", VaultMention::Vault)
        .expect("normalized selector");

    assert_eq!(target.profile_id, fixture.id("Café"));
}

#[test]
fn vault_target_resolution_uses_default_without_any_selector_signal() {
    let fixture = TargetFixture::new(&["Personal", "Work"], Some(1));

    let target = fixture
        .resolver()
        .resolve("@vault list recent notes", VaultMention::Vault)
        .expect("default target");

    assert_eq!(target.profile_id, fixture.id("Work"));
}

#[test]
fn vault_target_resolution_does_not_count_skill_token_as_profile_mention() {
    let fixture = TargetFixture::new(&["Vault", "Personal"], Some(1));

    let target = fixture
        .resolver()
        .resolve("@vault list recent notes", VaultMention::Vault)
        .expect("default target");

    assert_eq!(target.profile_id, fixture.id("Personal"));
}

#[test]
fn vault_target_resolution_accepts_immediate_quote_and_marker_forms() {
    let fixture = TargetFixture::new(&["Personal", "Work Notes"], Some(0));
    let resolver = fixture.resolver();

    let quoted = resolver
        .resolve("@vault \"Work Notes\" read todo.md", VaultMention::Vault)
        .expect("immediate quoted target");
    let marker = resolver
        .resolve("@vault read todo.md vault:Personal", VaultMention::Vault)
        .expect("unquoted marker target");
    let quoted_marker = resolver
        .resolve(
            "@vault search rust vault=\"Work Notes\"",
            VaultMention::Vault,
        )
        .expect("quoted marker target");

    assert_eq!(quoted.profile_id, fixture.id("Work Notes"));
    assert_eq!(marker.profile_id, fixture.id("Personal"));
    assert_eq!(quoted_marker.profile_id, fixture.id("Work Notes"));
}

#[test]
fn vault_target_resolution_prefers_longest_immediate_configured_prefix() {
    let fixture = TargetFixture::new(&["Work", "Work Notes"], Some(0));

    let target = fixture
        .resolver()
        .resolve("@vault Work Notes list projects", VaultMention::Vault)
        .expect("longest prefix target");

    assert_eq!(target.profile_id, fixture.id("Work Notes"));
}

#[test]
fn vault_target_resolution_selects_one_configured_name_mentioned_anywhere() {
    let fixture = TargetFixture::new(&["Personal", "Work Notes"], Some(0));

    let target = fixture
        .resolver()
        .resolve(
            "@vault find the roadmap in my work notes collection",
            VaultMention::Vault,
        )
        .expect("unique configured-name mention");

    assert_eq!(target.profile_id, fixture.id("Work Notes"));
}

#[test]
fn vault_target_resolution_rejects_unknown_marked_value_and_lists_names() {
    let fixture = TargetFixture::new(&["Personal", "Work Notes"], Some(0));

    let error = fixture
        .resolver()
        .resolve("@vault vault:Persnoal list", VaultMention::Vault)
        .expect_err("unknown marker must fail closed");

    assert!(matches!(
        error,
        VaultTargetError::UnknownExplicit { value, valid_names }
            if value == "Persnoal" && valid_names == ["Personal", "Work Notes"]
    ));
}

#[test]
fn vault_target_resolution_rejects_multiple_configured_mentions() {
    let fixture = TargetFixture::new(&["Personal", "Work Notes"], Some(0));

    let error = fixture
        .resolver()
        .resolve(
            "@vault compare Personal with Work Notes",
            VaultMention::Vault,
        )
        .expect_err("multiple targets must fail closed");

    assert!(matches!(
        error,
        VaultTargetError::Ambiguous { valid_names, .. }
            if valid_names == ["Personal", "Work Notes"]
    ));
}

#[test]
fn vault_target_resolution_rejects_missing_default_without_selector_signal() {
    let fixture = TargetFixture::new(&["Personal"], None);

    let error = fixture
        .resolver()
        .resolve("@vault list", VaultMention::Vault)
        .expect_err("missing default must fail closed");

    assert!(matches!(
        error,
        VaultTargetError::MissingDefault { valid_names } if valid_names == ["Personal"]
    ));
}

#[test]
fn vault_target_resolution_has_vault_and_obsidian_alias_parity() {
    let fixture = TargetFixture::new(&["Work Notes"], Some(0));
    let resolver = fixture.resolver();

    let vault = resolver
        .resolve("@vault \"Work Notes\" read todo.md", VaultMention::Vault)
        .expect("vault target");
    let obsidian = resolver
        .resolve(
            "@obsidian \"Work Notes\" read todo.md",
            VaultMention::Obsidian,
        )
        .expect("obsidian alias target");

    assert_eq!(vault, obsidian);
}
