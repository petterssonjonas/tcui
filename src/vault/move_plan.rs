// allow: SIZE_OK - T05A keeps the immutable move-plan domain and its security tests together.

use std::collections::HashSet;
use std::ffi::{OsStr, OsString};
use std::io::{self, Read};
use std::path::{Component, Path, PathBuf};
use std::time::SystemTime;

use blake3::Hash;
use cap_fs_ext::{DirExt, FollowSymlinks, MetadataExt as _, OpenOptionsFollowExt};
use cap_std::fs::{Dir, File, Metadata, OpenOptions};

use super::VaultRelativePath;

pub const MAX_MOVE_FILES: usize = 100;
pub const MAX_MOVE_BYTES: u64 = 256 * 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct VaultFileRef {
    path: VaultRelativePath,
}

impl VaultFileRef {
    pub fn new(path: VaultRelativePath) -> Self {
        Self { path }
    }

    pub fn path(&self) -> &VaultRelativePath {
        &self.path
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MovePair {
    source: VaultFileRef,
    destination: VaultFileRef,
    overwrite: bool,
}

impl MovePair {
    pub fn new(source: VaultFileRef, destination: VaultFileRef, overwrite: bool) -> Self {
        Self {
            source,
            destination,
            overwrite,
        }
    }

    pub fn source(&self) -> &VaultFileRef {
        &self.source
    }

    pub fn destination(&self) -> &VaultFileRef {
        &self.destination
    }

    pub fn overwrite(&self) -> bool {
        self.overwrite
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MoveFileKind {
    Directory,
    Symlink,
    Unsupported,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MoveCollision {
    Create,
    ExplicitOverwrite,
    UnapprovedCollision,
    InvalidType(MoveFileKind),
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct MovePlanId(String);

impl MovePlanId {
    fn new() -> Self {
        Self(format!("move-{:032x}", rand::random::<u128>()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FilePrecondition {
    pub size: u64,
    pub modified: SystemTime,
    pub blake3: Hash,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MovePlan {
    id: MovePlanId,
    pairs: Vec<MovePair>,
    preconditions: Vec<FilePrecondition>,
    collisions: Vec<MoveCollision>,
    total_size: u64,
    copy_only: bool,
}

impl MovePlan {
    pub fn build(
        pairs: Vec<MovePair>,
        data_root: &Path,
        is_open_in_editor: impl Fn(&VaultFileRef) -> bool,
    ) -> Result<Self, MovePlanError> {
        Self::build_transfer(pairs, data_root, is_open_in_editor, false)
    }

    pub fn build_copy(
        pairs: Vec<MovePair>,
        data_root: &Path,
        is_open_in_editor: impl Fn(&VaultFileRef) -> bool,
    ) -> Result<Self, MovePlanError> {
        Self::build_transfer(pairs, data_root, is_open_in_editor, true)
    }

    fn build_transfer(
        pairs: Vec<MovePair>,
        data_root: &Path,
        is_open_in_editor: impl Fn(&VaultFileRef) -> bool,
        copy_only: bool,
    ) -> Result<Self, MovePlanError> {
        validate_request(&pairs, &is_open_in_editor)?;
        let reader = VaultReader::open(data_root)?;
        let mut preconditions = Vec::with_capacity(pairs.len());
        let mut collisions = Vec::with_capacity(pairs.len());
        let mut total_size = 0_u64;

        for pair in &pairs {
            let remaining_bytes = MAX_MOVE_BYTES.saturating_sub(total_size);
            let precondition = reader.precondition(pair.source(), remaining_bytes)?;
            total_size = total_size.checked_add(precondition.size).ok_or(
                MovePlanError::TotalSizeExceeded {
                    actual: u64::MAX,
                    limit: MAX_MOVE_BYTES,
                },
            )?;
            if total_size > MAX_MOVE_BYTES {
                return Err(MovePlanError::TotalSizeExceeded {
                    actual: total_size,
                    limit: MAX_MOVE_BYTES,
                });
            }

            let collision = reader.classify_destination(pair)?;
            if let MoveCollision::InvalidType(_) = collision {
                return Err(MovePlanError::InvalidDestinationType {
                    destination: pair.destination.clone(),
                    collision,
                });
            }
            preconditions.push(precondition);
            collisions.push(collision);
        }

        validate_physical_targets(&pairs, &reader)?;
        Ok(Self {
            id: MovePlanId::new(),
            pairs,
            preconditions,
            collisions,
            total_size,
            copy_only,
        })
    }

    pub fn id(&self) -> &MovePlanId {
        &self.id
    }

    pub fn pairs(&self) -> &[MovePair] {
        &self.pairs
    }

    pub fn preconditions(&self) -> &[FilePrecondition] {
        &self.preconditions
    }

    pub fn collisions(&self) -> &[MoveCollision] {
        &self.collisions
    }

    pub fn total_size(&self) -> u64 {
        self.total_size
    }

    pub fn file_count(&self) -> usize {
        self.pairs.len()
    }

    pub fn copy_only(&self) -> bool {
        self.copy_only
    }

    pub fn revalidate(&self, data_root: &Path) -> Result<(), MovePlanError> {
        let reader = VaultReader::open(data_root)?;
        let mut total_size = 0_u64;
        for ((pair, expected), expected_collision) in self
            .pairs
            .iter()
            .zip(&self.preconditions)
            .zip(&self.collisions)
        {
            let remaining_bytes = MAX_MOVE_BYTES.saturating_sub(total_size);
            let actual = reader.precondition(pair.source(), remaining_bytes)?;
            if actual != *expected {
                return Err(MovePlanError::SourceChanged {
                    file: pair.source.clone(),
                });
            }
            total_size = total_size.saturating_add(actual.size);
            let actual_collision = reader.classify_destination(pair)?;
            if actual_collision != *expected_collision {
                return Err(MovePlanError::DestinationChanged {
                    destination: pair.destination.clone(),
                });
            }
        }
        validate_physical_targets(&self.pairs, &reader)?;
        Ok(())
    }
}

#[derive(Debug, thiserror::Error)]
pub enum MovePlanError {
    #[error("move plan must contain at least one file")]
    Empty,
    #[error("move plan contains {actual} files; the limit is {limit}")]
    TooManyFiles { actual: usize, limit: usize },
    #[error("move plan contains the same source more than once: {file:?}")]
    DuplicateSource { file: VaultFileRef },
    #[error("move plan contains the same destination more than once: {destination:?}")]
    DuplicateDestination { destination: VaultFileRef },
    #[error("move source and destination paths overlap: {file:?}")]
    OverlappingSourceDestination { file: VaultFileRef },
    #[error("move plan path is open in an editor: {file:?}")]
    OpenInEditor { file: VaultFileRef },
    #[error("failed to open internal Vault: {source}")]
    OpenRoot {
        #[source]
        source: io::Error,
    },
    #[error("move source does not exist: {file:?}")]
    MissingSource { file: VaultFileRef },
    #[error("move source has invalid type {kind:?}: {file:?}")]
    InvalidSourceType {
        file: VaultFileRef,
        kind: MoveFileKind,
    },
    #[error("move destination has invalid collision {collision:?}: {destination:?}")]
    InvalidDestinationType {
        destination: VaultFileRef,
        collision: MoveCollision,
    },
    #[error("move plan size is {actual} bytes; the limit is {limit}")]
    TotalSizeExceeded { actual: u64, limit: u64 },
    #[error("move source changed while planning or before execution: {file:?}")]
    SourceChanged { file: VaultFileRef },
    #[error("move destination changed after planning: {destination:?}")]
    DestinationChanged { destination: VaultFileRef },
    #[error("failed to {operation} move-plan path {file:?}: {source}")]
    Io {
        operation: &'static str,
        file: VaultFileRef,
        #[source]
        source: io::Error,
    },
}

fn validate_request(
    pairs: &[MovePair],
    is_open_in_editor: &impl Fn(&VaultFileRef) -> bool,
) -> Result<(), MovePlanError> {
    if pairs.is_empty() {
        return Err(MovePlanError::Empty);
    }
    if pairs.len() > MAX_MOVE_FILES {
        return Err(MovePlanError::TooManyFiles {
            actual: pairs.len(),
            limit: MAX_MOVE_FILES,
        });
    }

    let mut sources = HashSet::with_capacity(pairs.len());
    let mut destinations = HashSet::with_capacity(pairs.len());
    for pair in pairs {
        if !sources.insert(pair.source.clone()) {
            return Err(MovePlanError::DuplicateSource {
                file: pair.source.clone(),
            });
        }
        if !destinations.insert(pair.destination.clone()) {
            return Err(MovePlanError::DuplicateDestination {
                destination: pair.destination.clone(),
            });
        }
        for file in [&pair.source, &pair.destination] {
            if is_open_in_editor(file) {
                return Err(MovePlanError::OpenInEditor { file: file.clone() });
            }
        }
    }
    if let Some(file) = sources.iter().find(|source| destinations.contains(*source)) {
        return Err(MovePlanError::OverlappingSourceDestination { file: file.clone() });
    }
    Ok(())
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
enum PhysicalTarget {
    Entry {
        device: u64,
        inode: u64,
        suffix: PathBuf,
    },
    File {
        device: u64,
        inode: u64,
    },
}

fn validate_physical_targets(
    pairs: &[MovePair],
    reader: &VaultReader,
) -> Result<(), MovePlanError> {
    let mut sources = HashSet::with_capacity(pairs.len().saturating_mul(2));
    let mut destinations = HashSet::with_capacity(pairs.len().saturating_mul(2));
    for pair in pairs {
        for identity in reader.physical_target(pair.source())?.into_iter().flatten() {
            if !sources.insert(identity) {
                return Err(MovePlanError::DuplicateSource {
                    file: pair.source.clone(),
                });
            }
        }
        for identity in reader
            .physical_target(pair.destination())?
            .into_iter()
            .flatten()
        {
            if !destinations.insert(identity) {
                return Err(MovePlanError::DuplicateDestination {
                    destination: pair.destination.clone(),
                });
            }
        }
    }
    if sources
        .iter()
        .any(|identity| destinations.contains(identity))
    {
        return Err(MovePlanError::OverlappingSourceDestination {
            file: pairs[0].source.clone(),
        });
    }
    Ok(())
}

struct VaultReader {
    root: Dir,
    path: PathBuf,
}

impl VaultReader {
    fn open(data_root: &Path) -> Result<Self, MovePlanError> {
        let path = data_root.join("vault");
        super::require_root(&path).map_err(|source| MovePlanError::OpenRoot { source })?;
        crate::protection::open_directory(&path, false)
            .map(|root| Self { root, path })
            .map_err(|error| MovePlanError::OpenRoot {
                source: io::Error::other(error.to_string()),
            })
    }

    fn physical_target(
        &self,
        file: &VaultFileRef,
    ) -> Result<[Option<PhysicalTarget>; 2], MovePlanError> {
        super::require_root(&self.path.join(file.path().as_path()))
            .map_err(|error| io_error("unlock before resolving", file, error))?;
        let mut directory = self
            .root
            .try_clone()
            .map_err(|error| io_error("clone root", file, error))?;
        let mut components = file.path().as_path().components().peekable();
        while let Some(component) = components.next() {
            let name = normal_component(component, file)?;
            if components.peek().is_some() {
                match directory.open_dir_nofollow(name) {
                    Ok(child) => {
                        directory = child;
                        continue;
                    }
                    Err(error) if error.kind() == io::ErrorKind::NotFound => {
                        let mut suffix = PathBuf::from(name);
                        for component in components {
                            suffix.push(normal_component(component, file)?);
                        }
                        let metadata = directory
                            .metadata(".")
                            .map_err(|error| io_error("identify parent", file, error))?;
                        return Ok([
                            Some(PhysicalTarget::Entry {
                                device: metadata.dev(),
                                inode: metadata.ino(),
                                suffix,
                            }),
                            None,
                        ]);
                    }
                    Err(error) => return Err(io_error("resolve physical parent", file, error)),
                }
            }
            let parent = directory
                .metadata(".")
                .map_err(|error| io_error("identify parent", file, error))?;
            let entry = PhysicalTarget::Entry {
                device: parent.dev(),
                inode: parent.ino(),
                suffix: PathBuf::from(name),
            };
            let inode = match directory.symlink_metadata(name) {
                Ok(metadata) if metadata.is_file() => Some(PhysicalTarget::File {
                    device: metadata.dev(),
                    inode: metadata.ino(),
                }),
                Ok(_) => None,
                Err(error) if error.kind() == io::ErrorKind::NotFound => None,
                Err(error) => return Err(io_error("identify target", file, error)),
            };
            return Ok([Some(entry), inode]);
        }
        Err(io_error(
            "identify target",
            file,
            io::Error::other("empty target"),
        ))
    }

    fn precondition(
        &self,
        source: &VaultFileRef,
        remaining_bytes: u64,
    ) -> Result<FilePrecondition, MovePlanError> {
        let (parent, name) = match self.open_parent(source)? {
            ParentState::Found(parent, name) => (parent, name),
            ParentState::Missing => {
                return Err(MovePlanError::MissingSource {
                    file: source.clone(),
                });
            }
            ParentState::Invalid(kind) => {
                return Err(MovePlanError::InvalidSourceType {
                    file: source.clone(),
                    kind,
                });
            }
        };
        let entry_metadata = match parent.symlink_metadata(&name) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                return Err(MovePlanError::MissingSource {
                    file: source.clone(),
                });
            }
            Err(source_error) => return Err(io_error("inspect", source, source_error)),
        };
        if let Some(kind) = invalid_kind(&entry_metadata) {
            return Err(MovePlanError::InvalidSourceType {
                file: source.clone(),
                kind,
            });
        }

        let mut options = OpenOptions::new();
        options.read(true).follow(FollowSymlinks::No);
        let mut file = parent
            .open_with(&name, &options)
            .map_err(|source_error| io_error("open", source, source_error))?;
        let before = file
            .metadata()
            .map_err(|source_error| io_error("read metadata for", source, source_error))?;
        if let Some(kind) = invalid_kind(&before) {
            return Err(MovePlanError::InvalidSourceType {
                file: source.clone(),
                kind,
            });
        }
        let modified = before
            .modified()
            .map_err(|source_error| io_error("read modification time for", source, source_error))?
            .into_std();
        let overhead = if super::is_internal_root(&self.path.join(source.path().as_path())) {
            4096
        } else {
            0
        };
        if before.len() > remaining_bytes.saturating_add(overhead) {
            return Err(MovePlanError::TotalSizeExceeded {
                actual: MAX_MOVE_BYTES
                    .saturating_sub(remaining_bytes)
                    .saturating_add(before.len()),
                limit: MAX_MOVE_BYTES,
            });
        }
        let (digest, bytes_read, logical_size) =
            if super::is_internal_root(&self.path.join(source.path().as_path())) {
                let mut bytes = Vec::new();
                file.by_ref()
                    .take(remaining_bytes.saturating_add(overhead).saturating_add(1))
                    .read_to_end(&mut bytes)
                    .map_err(|error| io_error("read", source, error))?;
                let bytes_read = bytes.len() as u64;
                let decoded = super::decode_root(&self.path.join(source.path().as_path()), bytes)
                    .map_err(|error| io_error("decrypt", source, error))?;
                if decoded.len() as u64 > remaining_bytes {
                    return Err(MovePlanError::TotalSizeExceeded {
                        actual: MAX_MOVE_BYTES
                            .saturating_sub(remaining_bytes)
                            .saturating_add(decoded.len() as u64),
                        limit: MAX_MOVE_BYTES,
                    });
                }
                (blake3::hash(&decoded), bytes_read, decoded.len() as u64)
            } else {
                let (digest, bytes_read) = hash_file(&mut file, source, remaining_bytes)?;
                (digest, bytes_read, bytes_read)
            };
        let after = file
            .metadata()
            .map_err(|source_error| io_error("recheck metadata for", source, source_error))?;
        let after_modified = after
            .modified()
            .map_err(|source_error| {
                io_error("recheck modification time for", source, source_error)
            })?
            .into_std();
        if before.len() != after.len() || before.len() != bytes_read || modified != after_modified {
            return Err(MovePlanError::SourceChanged {
                file: source.clone(),
            });
        }
        Ok(FilePrecondition {
            size: logical_size,
            modified,
            blake3: digest,
        })
    }

    fn classify_destination(&self, pair: &MovePair) -> Result<MoveCollision, MovePlanError> {
        let (parent, name) = match self.open_parent(pair.destination())? {
            ParentState::Found(parent, name) => (parent, name),
            ParentState::Missing => return Ok(MoveCollision::Create),
            ParentState::Invalid(kind) => return Ok(MoveCollision::InvalidType(kind)),
        };
        let metadata = match parent.symlink_metadata(name) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                return Ok(MoveCollision::Create);
            }
            Err(source_error) => {
                return Err(io_error("inspect", pair.destination(), source_error));
            }
        };
        if let Some(kind) = invalid_kind(&metadata) {
            return Ok(MoveCollision::InvalidType(kind));
        }
        if pair.overwrite {
            Ok(MoveCollision::ExplicitOverwrite)
        } else {
            Ok(MoveCollision::UnapprovedCollision)
        }
    }

    fn open_parent(&self, file: &VaultFileRef) -> Result<ParentState, MovePlanError> {
        super::require_root(&self.path.join(file.path().as_path()))
            .map_err(|error| io_error("unlock before accessing", file, error))?;
        let mut directory = self
            .root
            .try_clone()
            .map_err(|source_error| io_error("clone root for", file, source_error))?;
        let path = file.path.as_path();
        let mut components = path.components().peekable();
        while let Some(component) = components.next() {
            let name = normal_component(component, file)?;
            if components.peek().is_none() {
                return Ok(ParentState::Found(directory, name.to_os_string()));
            }
            match directory.open_dir_nofollow(name) {
                Ok(child) => directory = child,
                Err(error) if error.kind() == io::ErrorKind::NotFound => {
                    return Ok(ParentState::Missing);
                }
                Err(source_error) => {
                    let kind = match directory.symlink_metadata(name) {
                        Ok(metadata) => invalid_kind(&metadata),
                        Err(_) => None,
                    };
                    if let Some(kind) = kind {
                        return Ok(ParentState::Invalid(kind));
                    }
                    return Err(io_error("open parent of", file, source_error));
                }
            }
        }
        Err(io_error(
            "parse",
            file,
            io::Error::new(io::ErrorKind::InvalidInput, "empty vault-relative path"),
        ))
    }
}

enum ParentState {
    Found(Dir, OsString),
    Missing,
    Invalid(MoveFileKind),
}

fn normal_component<'a>(
    component: Component<'a>,
    file: &VaultFileRef,
) -> Result<&'a OsStr, MovePlanError> {
    match component {
        Component::Normal(name) => Ok(name),
        Component::Prefix(_) | Component::RootDir | Component::CurDir | Component::ParentDir => {
            Err(io_error(
                "parse",
                file,
                io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "invalid vault-relative component",
                ),
            ))
        }
    }
}

fn invalid_kind(metadata: &Metadata) -> Option<MoveFileKind> {
    if metadata.is_file() {
        None
    } else if metadata.is_dir() {
        Some(MoveFileKind::Directory)
    } else if metadata.is_symlink() {
        Some(MoveFileKind::Symlink)
    } else {
        Some(MoveFileKind::Unsupported)
    }
}

fn hash_file(
    file: &mut File,
    source: &VaultFileRef,
    remaining_bytes: u64,
) -> Result<(Hash, u64), MovePlanError> {
    let mut hasher = blake3::Hasher::new();
    let mut buffer = [0_u8; 64 * 1024];
    let mut bytes_read = 0_u64;
    loop {
        let count = file
            .read(&mut buffer)
            .map_err(|source_error| io_error("hash", source, source_error))?;
        if count == 0 {
            return Ok((hasher.finalize(), bytes_read));
        }
        let count_u64 = u64::try_from(count).map_err(|_| {
            io_error(
                "measure hashed bytes for",
                source,
                io::Error::new(io::ErrorKind::InvalidData, "read size does not fit u64"),
            )
        })?;
        bytes_read = bytes_read.saturating_add(count_u64);
        if bytes_read > remaining_bytes {
            return Err(MovePlanError::TotalSizeExceeded {
                actual: MAX_MOVE_BYTES
                    .saturating_sub(remaining_bytes)
                    .saturating_add(bytes_read),
                limit: MAX_MOVE_BYTES,
            });
        }
        hasher.update(&buffer[..count]);
    }
}

fn io_error(operation: &'static str, file: &VaultFileRef, source: io::Error) -> MovePlanError {
    MovePlanError::Io {
        operation,
        file: file.clone(),
        source,
    }
}

#[cfg(test)]
mod vault_move_plan {
    use std::fs;
    use std::path::{Path, PathBuf};

    use crate::vault::VaultRelativePath;

    use super::{
        MoveCollision, MoveFileKind, MovePair, MovePlan, MovePlanError, VaultFileRef,
        MAX_MOVE_BYTES, MAX_MOVE_FILES,
    };

    struct Fixture {
        base: PathBuf,
        source_root: PathBuf,
        destination_root: PathBuf,
        data_root: PathBuf,
    }

    impl Fixture {
        fn new(label: &str) -> Self {
            let base = std::env::temp_dir().join(format!(
                "tcui-vault-move-plan-{label}-{}-{}",
                std::process::id(),
                rand::random::<u64>()
            ));
            let data_root = base.join("data");
            let source_root = data_root.join("vault/source");
            let destination_root = data_root.join("vault/destination");
            fs::create_dir_all(&source_root).expect("create source vault");
            fs::create_dir_all(&destination_root).expect("create destination vault");

            Self {
                base,
                source_root,
                destination_root,
                data_root,
            }
        }

        fn source_ref(&self, path: &str) -> VaultFileRef {
            file_ref(&format!("source/{path}"))
        }

        fn destination_ref(&self, path: &str) -> VaultFileRef {
            file_ref(&format!("destination/{path}"))
        }

        fn pair(&self, source: &str, destination: &str, overwrite: bool) -> MovePair {
            MovePair::new(
                self.source_ref(source),
                self.destination_ref(destination),
                overwrite,
            )
        }

        fn write_source(&self, path: &str, contents: &[u8]) {
            write_file(&self.source_root, path, contents);
        }

        fn write_destination(&self, path: &str, contents: &[u8]) {
            write_file(&self.destination_root, path, contents);
        }
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.base);
        }
    }

    fn file_ref(path: &str) -> VaultFileRef {
        VaultFileRef::new(
            VaultRelativePath::parse(Path::new(path)).expect("valid vault-relative test path"),
        )
    }

    fn write_file(root: &Path, path: &str, contents: &[u8]) {
        let target = root.join(path);
        if let Some(parent) = target.parent() {
            fs::create_dir_all(parent).expect("create fixture parent");
        }
        fs::write(target, contents).expect("write fixture file");
    }

    #[test]
    fn planning_is_read_only_and_captures_blake3_preconditions() {
        let _env_guard = crate::test_support::env_lock()
            .lock()
            .expect("environment lock");
        // Given
        let fixture = Fixture::new("read-only");
        fixture.write_source("note.md", b"source sentinel");
        fixture.write_destination("untouched.md", b"destination sentinel");
        let source_before = fs::read(fixture.source_root.join("note.md")).expect("read source");
        let destination_before = fs::read(fixture.destination_root.join("untouched.md"))
            .expect("read destination sentinel");

        // When
        let plan = MovePlan::build(
            vec![fixture.pair("note.md", "moved.md", false)],
            &fixture.data_root,
            |_| false,
        )
        .expect("build move plan");

        // Then
        assert_eq!(plan.file_count(), 1);
        assert_eq!(plan.total_size(), source_before.len() as u64);
        assert_eq!(plan.preconditions()[0].blake3, blake3::hash(&source_before));
        assert_eq!(
            fs::read(fixture.source_root.join("note.md")).expect("re-read source"),
            source_before
        );
        assert_eq!(
            fs::read(fixture.destination_root.join("untouched.md"))
                .expect("re-read destination sentinel"),
            destination_before
        );
        assert!(!fixture.destination_root.join("moved.md").exists());
    }

    #[test]
    fn file_count_limit_accepts_one_hundred_and_rejects_one_hundred_one() {
        let _env_guard = crate::test_support::env_lock()
            .lock()
            .expect("environment lock");
        // Given
        let fixture = Fixture::new("file-count");
        let mut pairs = Vec::new();
        for index in 0..=MAX_MOVE_FILES {
            let source = format!("source-{index}.md");
            let destination = format!("destination-{index}.md");
            fixture.write_source(&source, b"x");
            pairs.push(fixture.pair(&source, &destination, false));
        }

        // When
        let accepted =
            MovePlan::build(pairs[..MAX_MOVE_FILES].to_vec(), &fixture.data_root, |_| {
                false
            });
        let rejected = MovePlan::build(pairs, &fixture.data_root, |_| false);

        // Then
        assert_eq!(accepted.expect("100 files are allowed").file_count(), 100);
        assert!(matches!(
            rejected,
            Err(MovePlanError::TooManyFiles {
                actual: 101,
                limit: 100
            })
        ));
    }

    #[test]
    fn total_size_limit_accepts_256_mib_and_rejects_one_extra_byte() {
        let _env_guard = crate::test_support::env_lock()
            .lock()
            .expect("environment lock");
        // Given
        let fixture = Fixture::new("total-size");
        let exact = fixture.source_root.join("exact.md");
        let oversized = fixture.source_root.join("oversized.md");
        fs::File::create(&exact)
            .expect("create exact-size source")
            .set_len(MAX_MOVE_BYTES)
            .expect("set exact source size");
        fs::File::create(&oversized)
            .expect("create oversized source")
            .set_len(MAX_MOVE_BYTES + 1)
            .expect("set oversized source size");

        // When
        let accepted = MovePlan::build(
            vec![fixture.pair("exact.md", "exact.md", false)],
            &fixture.data_root,
            |_| false,
        );
        let rejected = MovePlan::build(
            vec![fixture.pair("oversized.md", "oversized.md", false)],
            &fixture.data_root,
            |_| false,
        );

        // Then
        assert_eq!(
            accepted.expect("256 MiB is allowed").total_size(),
            MAX_MOVE_BYTES
        );
        assert!(matches!(
            rejected,
            Err(MovePlanError::TotalSizeExceeded {
                actual,
                limit: MAX_MOVE_BYTES
            }) if actual == MAX_MOVE_BYTES + 1
        ));
    }

    #[test]
    fn source_directory_is_rejected() {
        let _env_guard = crate::test_support::env_lock()
            .lock()
            .expect("environment lock");
        // Given
        let fixture = Fixture::new("source-directory");
        fs::create_dir(fixture.source_root.join("folder")).expect("create source directory");

        // When
        let result = MovePlan::build(
            vec![fixture.pair("folder", "folder.md", false)],
            &fixture.data_root,
            |_| false,
        );

        // Then
        assert!(matches!(
            result,
            Err(MovePlanError::InvalidSourceType {
                kind: MoveFileKind::Directory,
                ..
            })
        ));
    }

    #[cfg(unix)]
    #[test]
    fn source_symlink_is_rejected_without_following_it() {
        use std::os::unix::fs::symlink;

        let _env_guard = crate::test_support::env_lock()
            .lock()
            .expect("environment lock");
        // Given
        let fixture = Fixture::new("source-symlink");
        let outside = fixture.base.join("outside.md");
        fs::write(&outside, b"outside sentinel").expect("write outside sentinel");
        symlink(&outside, fixture.source_root.join("linked.md")).expect("create source symlink");

        // When
        let result = MovePlan::build(
            vec![fixture.pair("linked.md", "moved.md", false)],
            &fixture.data_root,
            |_| false,
        );

        // Then
        assert!(matches!(
            result,
            Err(MovePlanError::InvalidSourceType {
                kind: MoveFileKind::Symlink,
                ..
            })
        ));
        assert_eq!(
            fs::read(outside).expect("read outside sentinel"),
            b"outside sentinel"
        );
    }

    #[cfg(unix)]
    #[test]
    fn unsupported_source_file_is_rejected() {
        use std::os::unix::net::UnixListener;

        let _env_guard = crate::test_support::env_lock()
            .lock()
            .expect("environment lock");
        // Given
        let fixture = Fixture::new("source-unsupported");
        let socket_path = fixture.source_root.join("socket.md");
        let _listener = UnixListener::bind(&socket_path).expect("bind fixture socket");

        // When
        let result = MovePlan::build(
            vec![fixture.pair("socket.md", "moved.md", false)],
            &fixture.data_root,
            |_| false,
        );

        // Then
        assert!(matches!(
            result,
            Err(MovePlanError::InvalidSourceType {
                kind: MoveFileKind::Unsupported,
                ..
            })
        ));
    }

    #[test]
    fn collisions_classify_create_explicit_overwrite_and_unapproved_collision() {
        let _env_guard = crate::test_support::env_lock()
            .lock()
            .expect("environment lock");
        // Given
        let fixture = Fixture::new("collisions");
        fixture.write_source("create.md", b"create");
        fixture.write_source("replace.md", b"replace");
        fixture.write_source("blocked.md", b"blocked");
        fixture.write_destination("replace.md", b"old replace");
        fixture.write_destination("blocked.md", b"old blocked");

        // When
        let plan = MovePlan::build(
            vec![
                fixture.pair("create.md", "create.md", false),
                fixture.pair("replace.md", "replace.md", true),
                fixture.pair("blocked.md", "blocked.md", false),
            ],
            &fixture.data_root,
            |_| false,
        )
        .expect("classify destination states");

        // Then
        assert_eq!(
            plan.collisions(),
            &[
                MoveCollision::Create,
                MoveCollision::ExplicitOverwrite,
                MoveCollision::UnapprovedCollision,
            ]
        );
    }

    #[test]
    fn invalid_destination_type_is_rejected_with_collision_classification() {
        let _env_guard = crate::test_support::env_lock()
            .lock()
            .expect("environment lock");
        // Given
        let fixture = Fixture::new("destination-directory");
        fixture.write_source("note.md", b"note");
        fs::create_dir(fixture.destination_root.join("folder")).expect("create destination folder");

        // When
        let result = MovePlan::build(
            vec![fixture.pair("note.md", "folder", false)],
            &fixture.data_root,
            |_| false,
        );

        // Then
        assert!(matches!(
            result,
            Err(MovePlanError::InvalidDestinationType {
                collision: MoveCollision::InvalidType(MoveFileKind::Directory),
                ..
            })
        ));
    }

    #[test]
    fn same_size_content_change_invalidates_whole_plan_by_digest() {
        let _env_guard = crate::test_support::env_lock()
            .lock()
            .expect("environment lock");
        // Given
        let fixture = Fixture::new("digest-change");
        fixture.write_source("note.md", b"alpha");
        let plan = MovePlan::build(
            vec![fixture.pair("note.md", "moved.md", false)],
            &fixture.data_root,
            |_| false,
        )
        .expect("build move plan");
        fs::write(fixture.source_root.join("note.md"), b"bravo").expect("modify source contents");

        // When
        let result = plan.revalidate(&fixture.data_root);

        // Then
        assert!(matches!(result, Err(MovePlanError::SourceChanged { .. })));
    }

    #[test]
    fn open_editor_source_or_destination_is_rejected_by_callback() {
        let _env_guard = crate::test_support::env_lock()
            .lock()
            .expect("environment lock");
        // Given
        let fixture = Fixture::new("open-editor");
        fixture.write_source("note.md", b"note");
        let pair = fixture.pair("note.md", "moved.md", false);
        let open_file = pair.source().clone();

        // When
        let result = MovePlan::build(vec![pair], &fixture.data_root, |candidate| {
            candidate == &open_file
        });

        // Then
        assert!(matches!(result, Err(MovePlanError::OpenInEditor { .. })));
    }

    #[test]
    fn duplicate_source_is_rejected() {
        let _env_guard = crate::test_support::env_lock()
            .lock()
            .expect("environment lock");
        // Given
        let fixture = Fixture::new("duplicate-source");
        fixture.write_source("note.md", b"note");

        // When
        let result = MovePlan::build(
            vec![
                fixture.pair("note.md", "first.md", false),
                fixture.pair("note.md", "second.md", false),
            ],
            &fixture.data_root,
            |_| false,
        );

        // Then
        assert!(matches!(result, Err(MovePlanError::DuplicateSource { .. })));
    }

    #[test]
    fn duplicate_destination_is_rejected() {
        let _env_guard = crate::test_support::env_lock()
            .lock()
            .expect("environment lock");
        // Given
        let fixture = Fixture::new("duplicate-destination");
        fixture.write_source("first.md", b"first");
        fixture.write_source("second.md", b"second");

        // When
        let result = MovePlan::build(
            vec![
                fixture.pair("first.md", "moved.md", false),
                fixture.pair("second.md", "moved.md", false),
            ],
            &fixture.data_root,
            |_| false,
        );

        // Then
        assert!(matches!(
            result,
            Err(MovePlanError::DuplicateDestination { .. })
        ));
    }

    #[test]
    fn same_directory_pair_with_distinct_paths_is_allowed() {
        let _env_guard = crate::test_support::env_lock()
            .lock()
            .expect("environment lock");
        // Given
        let fixture = Fixture::new("same-directory");
        fixture.write_source("note.md", b"note");
        let source = fixture.source_ref("note.md");
        let destination = fixture.source_ref("moved.md");

        // When
        let result = MovePlan::build(
            vec![MovePair::new(source, destination, false)],
            &fixture.data_root,
            |_| false,
        );

        // Then
        assert_eq!(result.expect("same-directory move plan").file_count(), 1);
    }

    #[test]
    fn native_plan_rejects_any_source_destination_overlap() {
        let _env_guard = crate::test_support::env_lock()
            .lock()
            .expect("environment lock");
        // Given
        let fixture = Fixture::new("native-overlap");
        fixture.write_source("first.md", b"first");
        fixture.write_source("second.md", b"second");
        let first = MovePair::new(
            fixture.source_ref("first.md"),
            fixture.source_ref("second.md"),
            true,
        );
        let second = MovePair::new(
            fixture.source_ref("second.md"),
            fixture.source_ref("third.md"),
            false,
        );

        // When
        let result = MovePlan::build(vec![first, second], &fixture.data_root, |_| false);

        // Then
        assert!(matches!(
            result,
            Err(MovePlanError::OverlappingSourceDestination { .. })
        ));
    }

    #[test]
    fn move_plan_ids_are_unique() {
        let _env_guard = crate::test_support::env_lock()
            .lock()
            .expect("environment lock");
        // Given
        let fixture = Fixture::new("plan-id");
        fixture.write_source("first.md", b"first");
        fixture.write_source("second.md", b"second");

        // When
        let first = MovePlan::build(
            vec![fixture.pair("first.md", "first.md", false)],
            &fixture.data_root,
            |_| false,
        )
        .expect("first plan");
        let second = MovePlan::build(
            vec![fixture.pair("second.md", "second.md", false)],
            &fixture.data_root,
            |_| false,
        )
        .expect("second plan");

        // Then
        assert_ne!(first.id(), second.id());
    }
}
