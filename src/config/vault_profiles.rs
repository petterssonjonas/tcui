//! Normalized Vault profile identity and catalog invariants.
// allow: SIZE_OK - T02A requires colocated domain tests in this sole-owner file.

use std::collections::{HashMap, HashSet};
use std::fmt;
use std::path::{Component, Path, PathBuf};
use std::str::FromStr;

use rand::RngCore;
use serde::{de::Error as _, Deserialize, Deserializer, Serialize, Serializer};
use unicode_normalization::UnicodeNormalization;

const VAULT_PROFILE_ID_BYTES: usize = 16;
const VAULT_PROFILE_ID_HEX_LENGTH: usize = VAULT_PROFILE_ID_BYTES * 2;

#[derive(Clone, PartialEq, Eq, Hash)]
pub struct VaultProfileId([u8; VAULT_PROFILE_ID_BYTES]);

impl VaultProfileId {
    pub fn new() -> Self {
        let mut bytes = [0_u8; VAULT_PROFILE_ID_BYTES];
        rand::rng().fill_bytes(&mut bytes);
        Self(bytes)
    }
}

impl fmt::Display for VaultProfileId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        for byte in self.0 {
            write!(formatter, "{byte:02x}")?;
        }
        Ok(())
    }
}

impl fmt::Debug for VaultProfileId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_tuple("VaultProfileId")
            .field(&self.to_string())
            .finish()
    }
}

impl FromStr for VaultProfileId {
    type Err = VaultProfileError;

    fn from_str(raw: &str) -> Result<Self, Self::Err> {
        if raw.len() != VAULT_PROFILE_ID_HEX_LENGTH
            || !raw
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        {
            return Err(VaultProfileError::InvalidId {
                value: raw.to_owned(),
            });
        }

        let mut decoded = [0_u8; VAULT_PROFILE_ID_BYTES];
        for (index, pair) in raw.as_bytes().chunks_exact(2).enumerate() {
            let high = decode_hex_nibble(pair[0]);
            let low = decode_hex_nibble(pair[1]);
            let (Some(high), Some(low)) = (high, low) else {
                return Err(VaultProfileError::InvalidId {
                    value: raw.to_owned(),
                });
            };
            decoded[index] = (high << 4) | low;
        }
        Ok(Self(decoded))
    }
}

impl Serialize for VaultProfileId {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.collect_str(self)
    }
}

impl<'de> Deserialize<'de> for VaultProfileId {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        String::deserialize(deserializer)?
            .parse()
            .map_err(D::Error::custom)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize)]
#[serde(transparent)]
pub struct VaultNameKey(String);

impl VaultNameKey {
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for VaultNameKey {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl<'de> Deserialize<'de> for VaultNameKey {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        Ok(normalize_name(&String::deserialize(deserializer)?))
    }
}

pub fn normalize_name(raw: &str) -> VaultNameKey {
    let normalized: String = raw.trim().nfkc().collect();
    let lowercase: String = normalized.chars().flat_map(char::to_lowercase).collect();
    VaultNameKey(lowercase.nfkc().collect())
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct VaultProfile {
    pub id: VaultProfileId,
    pub display_name: String,
    pub name_key: VaultNameKey,
    pub root: PathBuf,
    pub available: bool,
}

impl VaultProfile {
    pub fn new(display_name: &str, root: impl AsRef<Path>) -> Result<Self, VaultProfileError> {
        let (display_name, name_key) = validated_name(display_name)?;
        let root = existing_directory(root.as_ref())?;
        Ok(Self {
            id: VaultProfileId::new(),
            display_name,
            name_key,
            root,
            available: true,
        })
    }

    pub fn from_persisted(
        id: VaultProfileId,
        display_name: &str,
        root: impl AsRef<Path>,
    ) -> Result<Self, VaultProfileError> {
        let (display_name, name_key) = validated_name(display_name)?;
        let root = root.as_ref();
        let (root, available) = match std::fs::metadata(root) {
            Ok(metadata) if metadata.is_dir() => (existing_directory(root)?, true),
            Ok(_) | Err(_) => (lexical_absolute(root)?, false),
        };
        Ok(Self {
            id,
            display_name,
            name_key,
            root,
            available,
        })
    }
}

#[derive(Deserialize)]
struct PersistedVaultProfile {
    id: VaultProfileId,
    display_name: String,
    #[serde(rename = "name_key")]
    _name_key: VaultNameKey,
    root: PathBuf,
    #[serde(rename = "available")]
    _available: bool,
}

impl<'de> Deserialize<'de> for VaultProfile {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let persisted = PersistedVaultProfile::deserialize(deserializer)?;
        Self::from_persisted(persisted.id, &persisted.display_name, persisted.root)
            .map_err(D::Error::custom)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize)]
pub struct VaultCatalog {
    pub profiles: HashMap<VaultProfileId, VaultProfile>,
    pub default_id: Option<VaultProfileId>,
}

impl VaultCatalog {
    pub fn add(&mut self, profile: VaultProfile) -> Result<VaultProfileId, VaultProfileError> {
        if self.profiles.contains_key(&profile.id) {
            return Err(VaultProfileError::DuplicateId {
                id: profile.id.clone(),
            });
        }
        if self
            .profiles
            .values()
            .any(|existing| existing.name_key == profile.name_key)
        {
            return Err(VaultProfileError::DuplicateName {
                name_key: profile.name_key,
            });
        }
        if self
            .profiles
            .values()
            .any(|existing| existing.root == profile.root)
        {
            return Err(VaultProfileError::DuplicateRoot { root: profile.root });
        }

        let id = profile.id.clone();
        if self.profiles.is_empty() {
            self.default_id = Some(id.clone());
        }
        self.profiles.insert(id.clone(), profile);
        Ok(id)
    }

    pub fn remove(
        &mut self,
        id: &VaultProfileId,
    ) -> Result<Option<VaultProfile>, VaultProfileError> {
        if !self.profiles.contains_key(id) {
            return Ok(None);
        }
        if self.default_id.as_ref() == Some(id) && self.profiles.len() > 1 {
            return Err(VaultProfileError::InvalidDefault {
                id: Some(id.clone()),
            });
        }

        let removed = self.profiles.remove(id);
        if self.profiles.is_empty() {
            self.default_id = None;
        }
        Ok(removed)
    }

    pub fn rename(
        &mut self,
        id: &VaultProfileId,
        display_name: &str,
    ) -> Result<(), VaultProfileError> {
        let (display_name, name_key) = validated_name(display_name)?;
        if self
            .profiles
            .iter()
            .any(|(existing_id, profile)| existing_id != id && profile.name_key == name_key)
        {
            return Err(VaultProfileError::DuplicateName { name_key });
        }
        let Some(profile) = self.profiles.get_mut(id) else {
            return Err(VaultProfileError::InvalidDefault {
                id: Some(id.clone()),
            });
        };
        profile.display_name = display_name;
        profile.name_key = name_key;
        Ok(())
    }

    pub fn set_default(&mut self, id: &VaultProfileId) -> Result<(), VaultProfileError> {
        if !self.profiles.contains_key(id) {
            return Err(VaultProfileError::InvalidDefault {
                id: Some(id.clone()),
            });
        }
        self.default_id = Some(id.clone());
        Ok(())
    }

    pub fn get(&self, id: &VaultProfileId) -> Option<&VaultProfile> {
        self.profiles.get(id)
    }

    pub fn get_default(&self) -> Option<&VaultProfile> {
        self.default_id
            .as_ref()
            .and_then(|id| self.profiles.get(id))
    }

    pub fn find_by_name_key(&self, name_key: &VaultNameKey) -> Option<&VaultProfile> {
        self.profiles
            .values()
            .find(|profile| &profile.name_key == name_key)
    }

    pub fn validate(&self) -> Result<(), VaultProfileError> {
        let Some(default_id) = self.default_id.as_ref() else {
            return if self.profiles.is_empty() {
                Ok(())
            } else {
                Err(VaultProfileError::InvalidDefault { id: None })
            };
        };
        if self.profiles.is_empty() {
            return Err(VaultProfileError::NoProfilesWithDefault {
                id: default_id.clone(),
            });
        }
        if !self.profiles.contains_key(default_id) {
            return Err(VaultProfileError::InvalidDefault {
                id: Some(default_id.clone()),
            });
        }

        let mut ids = HashSet::with_capacity(self.profiles.len());
        let mut names = HashSet::with_capacity(self.profiles.len());
        let mut roots = HashSet::with_capacity(self.profiles.len());
        for (catalog_id, profile) in &self.profiles {
            if catalog_id != &profile.id || !ids.insert(&profile.id) {
                return Err(VaultProfileError::DuplicateId {
                    id: profile.id.clone(),
                });
            }
            let (_, normalized_name) = validated_name(&profile.display_name)?;
            if normalized_name != profile.name_key || !names.insert(&profile.name_key) {
                return Err(VaultProfileError::DuplicateName {
                    name_key: profile.name_key.clone(),
                });
            }
            if !roots.insert(&profile.root) {
                return Err(VaultProfileError::DuplicateRoot {
                    root: profile.root.clone(),
                });
            }
        }
        Ok(())
    }
}

#[derive(Deserialize)]
struct PersistedVaultCatalog {
    profiles: HashMap<VaultProfileId, VaultProfile>,
    default_id: Option<VaultProfileId>,
}

impl<'de> Deserialize<'de> for VaultCatalog {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let persisted = PersistedVaultCatalog::deserialize(deserializer)?;
        let catalog = Self {
            profiles: persisted.profiles,
            default_id: persisted.default_id,
        };
        catalog.validate().map_err(D::Error::custom)?;
        Ok(catalog)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum VaultProfileError {
    #[error("vault display name is empty")]
    EmptyName,
    #[error("vault name key already exists: {name_key}")]
    DuplicateName { name_key: VaultNameKey },
    #[error("vault root already exists: {root}")]
    DuplicateRoot { root: PathBuf },
    #[error("vault profile ID already exists: {id}")]
    DuplicateId { id: VaultProfileId },
    #[error("vault catalog default is invalid: {id:?}")]
    InvalidDefault { id: Option<VaultProfileId> },
    #[error("vault root is not an existing directory: {path}")]
    NotADirectory { path: PathBuf },
    #[error("empty vault catalog cannot have default profile: {id}")]
    NoProfilesWithDefault { id: VaultProfileId },
    #[error("vault profile ID must be 32 lowercase hexadecimal characters: {value}")]
    InvalidId { value: String },
}

fn decode_hex_nibble(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        _ => None,
    }
}

fn validated_name(raw: &str) -> Result<(String, VaultNameKey), VaultProfileError> {
    let display_name = raw.trim();
    let name_key = normalize_name(raw);
    if display_name.is_empty() || name_key.0.chars().all(is_ignorable_name_character) {
        return Err(VaultProfileError::EmptyName);
    }
    Ok((display_name.to_owned(), name_key))
}

fn is_ignorable_name_character(character: char) -> bool {
    character.is_whitespace()
        || matches!(
            character,
            '\u{00ad}'
                | '\u{034f}'
                | '\u{061c}'
                | '\u{115f}'
                | '\u{1160}'
                | '\u{17b4}'
                | '\u{17b5}'
                | '\u{180b}'..='\u{180f}'
                | '\u{200b}'..='\u{200f}'
                | '\u{202a}'..='\u{202e}'
                | '\u{2060}'..='\u{206f}'
                | '\u{3164}'
                | '\u{fe00}'..='\u{fe0f}'
                | '\u{feff}'
                | '\u{ffa0}'
        )
}

fn existing_directory(path: &Path) -> Result<PathBuf, VaultProfileError> {
    let metadata = std::fs::metadata(path).map_err(|_| VaultProfileError::NotADirectory {
        path: path.to_path_buf(),
    })?;
    if !metadata.is_dir() {
        return Err(VaultProfileError::NotADirectory {
            path: path.to_path_buf(),
        });
    }
    std::fs::canonicalize(path).map_err(|_| VaultProfileError::NotADirectory {
        path: path.to_path_buf(),
    })
}

fn lexical_absolute(path: &Path) -> Result<PathBuf, VaultProfileError> {
    let absolute = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()
            .map_err(|_| VaultProfileError::NotADirectory {
                path: path.to_path_buf(),
            })?
            .join(path)
    };
    let mut normalized = PathBuf::new();
    for component in absolute.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                normalized.pop();
            }
            Component::Prefix(prefix) => normalized.push(prefix.as_os_str()),
            Component::RootDir => normalized.push(component.as_os_str()),
            Component::Normal(part) => normalized.push(part),
        }
    }
    Ok(normalized)
}

#[cfg(test)]
mod vault_profile_domain {
    use std::path::{Path, PathBuf};
    use std::str::FromStr;

    use super::{normalize_name, VaultCatalog, VaultProfile, VaultProfileError, VaultProfileId};

    struct TestDir(PathBuf);

    impl TestDir {
        fn new(label: &str) -> Self {
            let path = std::env::temp_dir().join(format!(
                "tcui-vault-profile-{label}-{}-{}",
                std::process::id(),
                VaultProfileId::new()
            ));
            std::fs::create_dir_all(&path).expect("create test directory");
            Self(path)
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

    fn profile(name: &str, root: &Path) -> VaultProfile {
        VaultProfile::new(name, root).expect("valid test profile")
    }

    #[test]
    fn vault_profile_domain_normalizes_composed_and_decomposed_names_equally() {
        assert_eq!(normalize_name("Café"), normalize_name("Cafe\u{301}"));
    }

    #[test]
    fn vault_profile_domain_normalizes_nfkc_compatibility_equivalents_equally() {
        assert_eq!(normalize_name("ＶＡＵＬＴ"), normalize_name("vault"));
    }

    #[test]
    fn vault_profile_domain_applies_unicode_lowercase_expansion() {
        assert_eq!(normalize_name("İ"), normalize_name("i\u{307}"));
    }

    #[test]
    fn vault_profile_domain_normalizes_german_sharp_s_by_unicode_lowercase_rules() {
        assert_eq!(normalize_name("STRAẞE"), normalize_name("straße"));
    }

    #[test]
    fn vault_profile_domain_preserves_turkish_dotless_i_and_distinguishes_dotted_i() {
        assert_ne!(normalize_name("I"), normalize_name("ı"));
        assert_ne!(normalize_name("I"), normalize_name("İ"));
    }

    #[test]
    fn vault_profile_domain_rejects_empty_and_whitespace_only_names() {
        let root = TestDir::new("empty-name");

        assert_eq!(
            VaultProfile::new(" \t\n ", root.path()),
            Err(VaultProfileError::EmptyName)
        );
    }

    #[test]
    fn vault_profile_domain_rejects_zero_width_only_names() {
        let root = TestDir::new("zero-width-name");

        assert_eq!(
            VaultProfile::new("\u{200b}\u{200d}", root.path()),
            Err(VaultProfileError::EmptyName)
        );
    }

    #[test]
    fn vault_profile_domain_generates_fixed_width_lowercase_hex_ids() {
        let rendered = VaultProfileId::new().to_string();

        assert_eq!(rendered.len(), 32);
        assert!(rendered
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte)));
    }

    #[test]
    fn vault_profile_domain_generates_distinct_ids() {
        assert_ne!(VaultProfileId::new(), VaultProfileId::new());
    }

    #[test]
    fn vault_profile_domain_id_display_round_trips_without_regeneration() {
        let original = VaultProfileId::new();
        let parsed = VaultProfileId::from_str(&original.to_string()).expect("parse generated ID");

        assert_eq!(parsed, original);
    }

    #[test]
    fn vault_profile_domain_rejects_malformed_ids_with_typed_errors() {
        for malformed in [
            "abc",
            "ABCDEF0123456789ABCDEF0123456789",
            "gggggggggggggggggggggggggggggggg",
        ] {
            assert!(matches!(
                VaultProfileId::from_str(malformed),
                Err(VaultProfileError::InvalidId { .. })
            ));
        }
    }

    #[test]
    fn vault_profile_domain_rejects_duplicate_normalized_names() {
        let first_root = TestDir::new("duplicate-name-first");
        let second_root = TestDir::new("duplicate-name-second");
        let mut catalog = VaultCatalog::default();
        catalog
            .add(profile("Café", first_root.path()))
            .expect("add first profile");

        let error = catalog
            .add(profile(" Cafe\u{301} ", second_root.path()))
            .expect_err("reject duplicate normalized name");

        assert!(matches!(error, VaultProfileError::DuplicateName { .. }));
    }

    #[test]
    fn vault_profile_domain_rejects_duplicate_canonical_roots() {
        let parent = TestDir::new("duplicate-root");
        let root = parent.path().join("vault");
        std::fs::create_dir(&root).expect("create vault directory");
        let alias = root.join("..").join("vault");
        let mut catalog = VaultCatalog::default();
        catalog
            .add(profile("First", &root))
            .expect("add first profile");

        let error = catalog
            .add(profile("Second", &alias))
            .expect_err("reject duplicate canonical root");

        assert!(matches!(error, VaultProfileError::DuplicateRoot { .. }));
    }

    #[test]
    fn vault_profile_domain_rejects_duplicate_ids() {
        let first_root = TestDir::new("duplicate-id-first");
        let second_root = TestDir::new("duplicate-id-second");
        let mut catalog = VaultCatalog::default();
        let first = profile("First", first_root.path());
        let duplicate =
            VaultProfile::from_persisted(first.id.clone(), "Second", second_root.path())
                .expect("create persisted duplicate");
        catalog.add(first).expect("add first profile");

        let error = catalog.add(duplicate).expect_err("reject duplicate ID");

        assert!(matches!(error, VaultProfileError::DuplicateId { .. }));
    }

    #[test]
    fn vault_profile_domain_requires_reassigning_default_before_removal() {
        let first_root = TestDir::new("remove-default-first");
        let second_root = TestDir::new("remove-default-second");
        let mut catalog = VaultCatalog::default();
        let first_id = catalog
            .add(profile("First", first_root.path()))
            .expect("add first profile")
            .clone();
        catalog
            .add(profile("Second", second_root.path()))
            .expect("add second profile");

        let error = catalog
            .remove(&first_id)
            .expect_err("cannot remove active default");

        assert!(matches!(error, VaultProfileError::InvalidDefault { .. }));
    }

    #[test]
    fn vault_profile_domain_removes_old_default_after_reassignment() {
        let first_root = TestDir::new("reassign-default-first");
        let second_root = TestDir::new("reassign-default-second");
        let mut catalog = VaultCatalog::default();
        let first_id = catalog
            .add(profile("First", first_root.path()))
            .expect("add first profile")
            .clone();
        let second_id = catalog
            .add(profile("Second", second_root.path()))
            .expect("add second profile")
            .clone();
        catalog
            .set_default(&second_id)
            .expect("set replacement default");

        let removed = catalog
            .remove(&first_id)
            .expect("remove old default")
            .expect("profile existed");

        assert_eq!(removed.id, first_id);
    }

    #[test]
    fn vault_profile_domain_removing_only_profile_clears_default() {
        let root = TestDir::new("remove-only");
        let mut catalog = VaultCatalog::default();
        let id = catalog
            .add(profile("Only", root.path()))
            .expect("add profile")
            .clone();

        catalog.remove(&id).expect("remove only profile");

        assert!(catalog.default_id.is_none());
    }

    #[test]
    fn vault_profile_domain_rejects_unknown_default() {
        let root = TestDir::new("unknown-default");
        let mut catalog = VaultCatalog::default();
        catalog
            .add(profile("Only", root.path()))
            .expect("add profile");

        let error = catalog
            .set_default(&VaultProfileId::new())
            .expect_err("reject unknown default");

        assert!(matches!(error, VaultProfileError::InvalidDefault { .. }));
    }

    #[test]
    fn vault_profile_domain_catalog_with_profiles_requires_default() {
        let root = TestDir::new("missing-default");
        let mut catalog = VaultCatalog::default();
        catalog
            .add(profile("Only", root.path()))
            .expect("add profile");
        catalog.default_id = None;

        assert_eq!(
            catalog.validate(),
            Err(VaultProfileError::InvalidDefault { id: None })
        );
    }

    #[test]
    fn vault_profile_domain_empty_catalog_has_no_default() {
        let catalog = VaultCatalog::default();

        assert!(catalog.get_default().is_none());
        assert_eq!(catalog.validate(), Ok(()));
    }

    #[test]
    fn vault_profile_domain_empty_catalog_rejects_configured_default() {
        let id = VaultProfileId::new();
        let catalog = VaultCatalog {
            profiles: std::collections::HashMap::new(),
            default_id: Some(id.clone()),
        };

        assert_eq!(
            catalog.validate(),
            Err(VaultProfileError::NoProfilesWithDefault { id })
        );
    }

    #[test]
    fn vault_profile_domain_rename_recomputes_name_key() {
        let root = TestDir::new("rename");
        let mut catalog = VaultCatalog::default();
        let id = catalog
            .add(profile("Before", root.path()))
            .expect("add profile")
            .clone();

        catalog.rename(&id, " After ").expect("rename profile");

        assert_eq!(
            catalog.get(&id).map(|profile| &profile.name_key),
            Some(&normalize_name("after"))
        );
    }

    #[test]
    fn vault_profile_domain_new_profile_rejects_nonexistent_absolute_path() {
        let missing = std::env::temp_dir()
            .join("tcui-vault-profile-missing")
            .join(VaultProfileId::new().to_string());

        let error = VaultProfile::new("Missing", &missing).expect_err("reject missing path");

        assert_eq!(error, VaultProfileError::NotADirectory { path: missing });
    }

    #[test]
    fn vault_profile_domain_parent_components_are_canonicalized() {
        let parent = TestDir::new("parent-components");
        let root = parent.path().join("vault");
        std::fs::create_dir(&root).expect("create vault directory");

        let profile = profile("Vault", &root.join("..").join("vault"));

        assert_eq!(
            profile.root,
            std::fs::canonicalize(root).expect("canonical root")
        );
    }

    #[test]
    fn vault_profile_domain_persisted_missing_path_loads_as_unavailable() {
        let root = TestDir::new("persisted-unavailable");
        let profile = profile("Vault", root.path());
        let encoded = serde_json::to_string(&profile).expect("serialize profile");
        std::fs::remove_dir_all(root.path()).expect("remove persisted root");

        let loaded: VaultProfile = serde_json::from_str(&encoded).expect("deserialize profile");

        assert!(!loaded.available);
    }

    #[test]
    fn vault_profile_domain_catalog_round_trips_through_serde() {
        let first_root = TestDir::new("round-trip-first");
        let second_root = TestDir::new("round-trip-second");
        let mut catalog = VaultCatalog::default();
        catalog
            .add(profile("First", first_root.path()))
            .expect("add first profile");
        catalog
            .add(profile("Second", second_root.path()))
            .expect("add second profile");
        let encoded = serde_json::to_string(&catalog).expect("serialize catalog");

        let decoded: VaultCatalog = serde_json::from_str(&encoded).expect("deserialize catalog");

        assert_eq!(decoded, catalog);
    }
}
