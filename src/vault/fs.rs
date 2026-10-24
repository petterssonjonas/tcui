use std::ffi::{OsStr, OsString};
use std::io::{self, Read};
use std::path::{Component, Path, PathBuf};

use cap_fs_ext::{DirExt, FollowSymlinks, OpenOptionsFollowExt};
use cap_std::fs::{Dir, File, OpenOptions};

use super::{
    EditorTargetError, ValidatedEditorTarget, VaultLimitKind, VaultLimitReached, VaultRelativePath,
    VaultSearchLimits, VaultWalkLimits,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum VaultEntryKind {
    Directory,
    File,
    UnsupportedSymlink,
    Unsupported,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VaultEntry {
    pub path: PathBuf,
    pub kind: VaultEntryKind,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VaultListing {
    pub entries: Vec<VaultEntry>,
    pub limits: Vec<VaultLimitReached>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VaultTextRead {
    pub text: String,
    pub limits: Vec<VaultLimitReached>,
}

impl VaultListing {
    pub fn is_complete(&self) -> bool {
        self.limits.is_empty()
    }

    pub fn limit_reached(&self, kind: VaultLimitKind) -> bool {
        self.limits.iter().any(|limit| limit.kind == kind)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VaultSearchMatch {
    pub path: PathBuf,
    pub byte_offset: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VaultSearchResult {
    pub matches: Vec<VaultSearchMatch>,
    pub files_scanned: usize,
    pub total_bytes_read: u64,
    pub limits: Vec<VaultLimitReached>,
}

impl VaultSearchResult {
    pub fn is_complete(&self) -> bool {
        self.limits.is_empty()
    }

    pub fn limit_reached(&self, kind: VaultLimitKind) -> bool {
        self.limits.iter().any(|limit| limit.kind == kind)
    }
}

#[derive(Debug, thiserror::Error)]
pub enum VaultFsError {
    #[error("failed to open vault profile {path}: {source}")]
    OpenProfile {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    #[error("vault path points through a symbolic link: {path}")]
    Symlink { path: PathBuf },
    #[error("vault path is not a regular file: {path}")]
    NotFile { path: PathBuf },
    #[error("vault path is not a directory: {path}")]
    NotDirectory { path: PathBuf },
    #[error("vault path invariant was violated: {path}")]
    InvalidRelativePath { path: PathBuf },
    #[error("vault file is not UTF-8 text: {path}")]
    InvalidUtf8 {
        path: PathBuf,
        #[source]
        source: std::string::FromUtf8Error,
    },
    #[error("vault search query cannot be empty")]
    EmptyQuery,
    #[error("failed to {operation} vault path {path}: {source}")]
    Io {
        operation: &'static str,
        path: PathBuf,
        #[source]
        source: io::Error,
    },
}

pub struct VaultFs(Dir, PathBuf);

impl VaultFs {
    pub fn open_profile(root: &Path) -> Result<Self, VaultFsError> {
        let absolute = if root.is_absolute() {
            root.to_path_buf()
        } else {
            std::env::current_dir()
                .map_err(|source| VaultFsError::OpenProfile {
                    path: root.to_path_buf(),
                    source,
                })?
                .join(root)
        };
        crate::protection::open_directory(&absolute, false)
            .map(|directory| Self(directory, absolute))
            .map_err(|error| VaultFsError::OpenProfile {
                path: root.to_path_buf(),
                source: io::Error::other(error.to_string()),
            })
    }

    pub fn read_bytes(&self, path: &VaultRelativePath) -> Result<Vec<u8>, VaultFsError> {
        let path = path.as_path();
        let mut file = self.open_file_nofollow(path)?;
        let mut bytes = Vec::new();
        file.read_to_end(&mut bytes)
            .map_err(|source| io_error("read", path, source))?;
        super::decode_root(&self.1.join(path), bytes)
            .map_err(|source| io_error("decrypt", path, source))
    }

    pub fn read_text(&self, path: &VaultRelativePath) -> Result<String, VaultFsError> {
        String::from_utf8(self.read_bytes(path)?).map_err(|source| VaultFsError::InvalidUtf8 {
            path: path.as_path().to_path_buf(),
            source,
        })
    }

    pub(crate) fn create_parent_directories(
        &self,
        path: &VaultRelativePath,
    ) -> Result<(), VaultFsError> {
        super::require_root(&self.1.join(path.as_path())).map_err(|source| {
            io_error("unlock before creating directories", path.as_path(), source)
        })?;
        let mut directory = self
            .0
            .try_clone()
            .map_err(|source| io_error("clone root", path.as_path(), source))?;
        if let Some(parent) = path.as_path().parent() {
            let mut traversed = PathBuf::new();
            for component in parent.components() {
                let name = normal_component(component, path.as_path())?;
                traversed.push(name);
                match directory.create_dir(name) {
                    Ok(()) => {}
                    Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
                    Err(source) => return Err(io_error("create directory", &traversed, source)),
                }
                directory = open_dir_component(&directory, name, &traversed)?;
            }
        }
        Ok(())
    }

    pub fn read_text_limited(
        &self,
        path: &VaultRelativePath,
        max_bytes: u64,
    ) -> Result<VaultTextRead, VaultFsError> {
        let (bytes, partial) = self.read_limited_bytes(path, max_bytes)?;
        let path = path.as_path();
        let text = match String::from_utf8(bytes) {
            Ok(text) => text,
            Err(error) if partial && error.utf8_error().error_len().is_none() => {
                let valid = error.utf8_error().valid_up_to();
                let mut bytes = error.into_bytes();
                bytes.truncate(valid);
                String::from_utf8(bytes).map_err(|source| VaultFsError::InvalidUtf8 {
                    path: path.to_path_buf(),
                    source,
                })?
            }
            Err(source) => {
                return Err(VaultFsError::InvalidUtf8 {
                    path: path.to_path_buf(),
                    source,
                });
            }
        };
        let limits = if partial {
            vec![VaultLimitReached::new(
                VaultLimitKind::FileBytes,
                Some(path.to_path_buf()),
            )]
        } else {
            Vec::new()
        };
        Ok(VaultTextRead { text, limits })
    }

    fn read_limited_bytes(
        &self,
        path: &VaultRelativePath,
        max_bytes: u64,
    ) -> Result<(Vec<u8>, bool), VaultFsError> {
        let path = path.as_path();
        let mut file = self.open_file_nofollow(path)?;
        let protected = super::is_internal_root(&self.1.join(path))
            && crate::protection::is_protected(crate::protection::Domain::Vault).map_err(
                |error| {
                    io_error(
                        "check protection",
                        path,
                        io::Error::other(error.to_string()),
                    )
                },
            )?;
        let overhead = if protected { 4096 } else { 0 };
        let mut bytes = Vec::new();
        file.by_ref()
            .take(max_bytes.saturating_add(overhead).saturating_add(1))
            .read_to_end(&mut bytes)
            .map_err(|source| io_error("read", path, source))?;
        if protected && bytes.len() as u64 > max_bytes.saturating_add(overhead) {
            return Ok((Vec::new(), true));
        }
        let mut bytes = super::decode_root(&self.1.join(path), bytes)
            .map_err(|source| io_error("decrypt", path, source))?;
        let bound = usize::try_from(max_bytes).unwrap_or(usize::MAX);
        let partial = bytes.len() > bound;
        bytes.truncate(bound);
        Ok((bytes, partial))
    }

    pub(crate) fn entry_kind(
        &self,
        path: &VaultRelativePath,
    ) -> Result<Option<VaultEntryKind>, VaultFsError> {
        let path = path.as_path();
        let (directory, name) = self.open_parent(path)?;
        let metadata = match directory.symlink_metadata(&name) {
            Ok(metadata) => metadata,
            Err(source) if source.kind() == io::ErrorKind::NotFound => return Ok(None),
            Err(source) => return Err(io_error("read metadata for", path, source)),
        };
        let kind = if metadata.is_dir() {
            VaultEntryKind::Directory
        } else if metadata.is_file() {
            VaultEntryKind::File
        } else if metadata.is_symlink() {
            VaultEntryKind::UnsupportedSymlink
        } else {
            VaultEntryKind::Unsupported
        };
        Ok(Some(kind))
    }

    pub(crate) fn validate_editor_target(
        &self,
        canonical_root: &Path,
        path: &VaultRelativePath,
    ) -> Result<ValidatedEditorTarget, EditorTargetError> {
        if super::is_internal_root(&self.1.join(path.as_path()))
            && crate::protection::is_protected(crate::protection::Domain::Vault).map_err(
                |error| EditorTargetError::Io {
                    operation: "check protection for",
                    path: path.as_path().to_path_buf(),
                    source: io::Error::other(error.to_string()),
                },
            )?
        {
            return Err(EditorTargetError::Io {
                operation: "open encrypted file; explicitly export plaintext first",
                path: path.as_path().to_path_buf(),
                source: io::Error::other("Protected files cannot be opened in an external editor"),
            });
        }
        super::editor::validate_editor_target(canonical_root, &self.0, path)
    }

    pub fn list(
        &self,
        start: Option<&VaultRelativePath>,
        limits: VaultWalkLimits,
    ) -> Result<VaultListing, VaultFsError> {
        super::require_root(&self.1).map_err(|source| {
            io_error("unlock with /unlock before listing", Path::new("."), source)
        })?;
        let (directory, prefix) = match start {
            Some(path) => (
                self.open_directory(path.as_path())?,
                path.as_path().to_path_buf(),
            ),
            None => (
                self.0.try_clone().map_err(|source| {
                    io_error("clone root directory handle", Path::new("."), source)
                })?,
                PathBuf::new(),
            ),
        };
        let mut state = WalkState::new(limits);
        self.walk_directory(&directory, &prefix, 0, &mut state)?;
        Ok(VaultListing {
            entries: state.entries,
            limits: state.reached,
        })
    }

    pub fn search(
        &self,
        query: &str,
        limits: VaultSearchLimits,
    ) -> Result<VaultSearchResult, VaultFsError> {
        if query.is_empty() {
            return Err(VaultFsError::EmptyQuery);
        }
        let listing = self.list(None, VaultWalkLimits::default())?;
        let mut result = VaultSearchResult {
            matches: Vec::new(),
            files_scanned: 0,
            total_bytes_read: 0,
            limits: listing.limits,
        };
        let folded_query = query.to_lowercase();

        for entry in listing.entries {
            if entry.kind != VaultEntryKind::File {
                continue;
            }
            if result.files_scanned == limits.max_files {
                record_limit(&mut result.limits, VaultLimitKind::Files, Some(entry.path));
                break;
            }
            result.files_scanned += 1;
            let remaining = limits
                .max_total_bytes
                .saturating_sub(result.total_bytes_read);
            let bound = limits.max_file_bytes.min(remaining);
            let relative = VaultRelativePath::parse(&entry.path).map_err(|_| {
                VaultFsError::InvalidRelativePath {
                    path: entry.path.clone(),
                }
            })?;
            let (bytes, partial) = self.read_limited_bytes(&relative, bound)?;
            if partial {
                let kind = if remaining < limits.max_file_bytes {
                    VaultLimitKind::TotalBytes
                } else {
                    VaultLimitKind::FileBytes
                };
                record_limit(&mut result.limits, kind, Some(entry.path));
                if kind == VaultLimitKind::TotalBytes {
                    break;
                }
                continue;
            }
            result.total_bytes_read += bytes.len() as u64;
            let Ok(text) = String::from_utf8(bytes) else {
                continue;
            };
            let folded_text = text.to_lowercase();
            for (byte_offset, _) in folded_text.match_indices(&folded_query) {
                if result.matches.len() == limits.max_matches {
                    record_limit(
                        &mut result.limits,
                        VaultLimitKind::Matches,
                        Some(entry.path),
                    );
                    return Ok(result);
                }
                result.matches.push(VaultSearchMatch {
                    path: entry.path.clone(),
                    byte_offset,
                });
            }
        }
        Ok(result)
    }

    fn open_directory(&self, path: &Path) -> Result<Dir, VaultFsError> {
        super::require_root(&self.1.join(path))
            .map_err(|source| io_error("unlock before listing", path, source))?;
        let mut directory = self
            .0
            .try_clone()
            .map_err(|source| io_error("clone root directory handle", path, source))?;
        let mut traversed = PathBuf::new();
        for component in path.components() {
            let name = normal_component(component, path)?;
            traversed.push(name);
            directory = open_dir_component(&directory, name, &traversed)?;
        }
        Ok(directory)
    }

    fn open_file_nofollow(&self, path: &Path) -> Result<File, VaultFsError> {
        super::require_root(&self.1.join(path))
            .map_err(|source| io_error("unlock with /unlock before reading", path, source))?;
        let (directory, name) = self.open_parent(path)?;
        let mut options = OpenOptions::new();
        options.read(true).follow(FollowSymlinks::No);
        let file = directory
            .open_with(&name, &options)
            .map_err(|source| classify_open_error(&directory, &name, path, "open", source))?;
        let metadata = file
            .metadata()
            .map_err(|source| io_error("read metadata for", path, source))?;
        if !metadata.is_file() {
            return Err(VaultFsError::NotFile {
                path: path.to_path_buf(),
            });
        }
        Ok(file)
    }

    fn open_parent(&self, path: &Path) -> Result<(Dir, OsString), VaultFsError> {
        super::require_root(&self.1.join(path))
            .map_err(|source| io_error("unlock with /unlock before accessing", path, source))?;
        let mut directory = self
            .0
            .try_clone()
            .map_err(|source| io_error("clone root directory handle", path, source))?;
        let mut components = path.components().peekable();
        let mut traversed = PathBuf::new();

        while let Some(component) = components.next() {
            let name = normal_component(component, path)?;
            if components.peek().is_none() {
                return Ok((directory, name.to_os_string()));
            }
            traversed.push(name);
            directory = open_dir_component(&directory, name, &traversed)?;
        }
        Err(VaultFsError::InvalidRelativePath {
            path: path.to_path_buf(),
        })
    }

    fn walk_directory(
        &self,
        directory: &Dir,
        prefix: &Path,
        depth: usize,
        state: &mut WalkState,
    ) -> Result<(), VaultFsError> {
        super::require_root(&self.1.join(prefix))
            .map_err(|source| io_error("unlock before listing", prefix, source))?;
        if depth >= state.limits.max_depth {
            state.record(VaultLimitKind::Depth, Some(prefix.to_path_buf()));
            return Ok(());
        }
        let mut entries = read_sorted_entries(directory, prefix)?;
        entries.sort_by(|left, right| {
            left.kind
                .cmp(&right.kind)
                .then_with(|| left.name.cmp(&right.name))
        });

        for entry in entries {
            if state.entries.len() == state.limits.max_entries {
                state.record(VaultLimitKind::Entries, Some(prefix.join(entry.name)));
                return Ok(());
            }
            let path = prefix.join(&entry.name);
            state.entries.push(VaultEntry {
                path: path.clone(),
                kind: entry.kind,
            });
            if entry.kind != VaultEntryKind::Directory {
                continue;
            }
            let entry_depth = depth.saturating_add(1);
            if entry_depth >= state.limits.max_depth {
                state.record(VaultLimitKind::Depth, Some(path));
                continue;
            }
            let child = open_dir_component(directory, &entry.name, &path)?;
            self.walk_directory(&child, &path, entry_depth, state)?;
            if state.limit_reached(VaultLimitKind::Entries) {
                return Ok(());
            }
        }
        Ok(())
    }
}

struct WalkState {
    limits: VaultWalkLimits,
    entries: Vec<VaultEntry>,
    reached: Vec<VaultLimitReached>,
}

impl WalkState {
    fn new(limits: VaultWalkLimits) -> Self {
        Self {
            limits,
            entries: Vec::new(),
            reached: Vec::new(),
        }
    }

    fn record(&mut self, kind: VaultLimitKind, path: Option<PathBuf>) {
        record_limit(&mut self.reached, kind, path);
    }

    fn limit_reached(&self, kind: VaultLimitKind) -> bool {
        self.reached.iter().any(|limit| limit.kind == kind)
    }
}

struct ScannedEntry {
    name: OsString,
    kind: VaultEntryKind,
}

fn read_sorted_entries(directory: &Dir, prefix: &Path) -> Result<Vec<ScannedEntry>, VaultFsError> {
    let iterator = directory
        .entries()
        .map_err(|source| io_error("list", prefix, source))?;
    let mut entries = Vec::new();
    for entry in iterator {
        let entry = entry.map_err(|source| io_error("list", prefix, source))?;
        let name = entry.file_name();
        if is_hidden(&name) {
            continue;
        }
        let file_type = entry
            .file_type()
            .map_err(|source| io_error("read entry type for", &prefix.join(&name), source))?;
        let kind = if file_type.is_dir() {
            VaultEntryKind::Directory
        } else if file_type.is_file() {
            VaultEntryKind::File
        } else if file_type.is_symlink() {
            VaultEntryKind::UnsupportedSymlink
        } else {
            VaultEntryKind::Unsupported
        };
        entries.push(ScannedEntry { name, kind });
    }
    Ok(entries)
}

fn normal_component<'a>(component: Component<'a>, path: &Path) -> Result<&'a OsStr, VaultFsError> {
    match component {
        Component::Normal(name) => Ok(name),
        Component::Prefix(_) | Component::RootDir | Component::CurDir | Component::ParentDir => {
            Err(VaultFsError::InvalidRelativePath {
                path: path.to_path_buf(),
            })
        }
    }
}

fn open_dir_component(directory: &Dir, name: &OsStr, path: &Path) -> Result<Dir, VaultFsError> {
    directory
        .open_dir_nofollow(name)
        .map_err(|source| classify_open_error(directory, name, path, "open directory", source))
}

fn classify_open_error(
    directory: &Dir,
    name: &OsStr,
    path: &Path,
    operation: &'static str,
    source: io::Error,
) -> VaultFsError {
    match directory.symlink_metadata(name) {
        Ok(metadata) if metadata.is_symlink() => VaultFsError::Symlink {
            path: path.to_path_buf(),
        },
        Ok(metadata) if !metadata.is_dir() && operation == "open directory" => {
            VaultFsError::NotDirectory {
                path: path.to_path_buf(),
            }
        }
        Ok(_) | Err(_) => io_error(operation, path, source),
    }
}

fn io_error(operation: &'static str, path: &Path, source: io::Error) -> VaultFsError {
    VaultFsError::Io {
        operation,
        path: path.to_path_buf(),
        source,
    }
}

fn is_hidden(name: &OsStr) -> bool {
    name.as_encoded_bytes().first() == Some(&b'.')
}

fn record_limit(reached: &mut Vec<VaultLimitReached>, kind: VaultLimitKind, path: Option<PathBuf>) {
    if !reached.iter().any(|limit| limit.kind == kind) {
        reached.push(VaultLimitReached::new(kind, path));
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::{Path, PathBuf};
    use std::sync::atomic::{AtomicU64, Ordering};

    use super::{VaultEntryKind, VaultFs, VaultFsError};
    use crate::vault::{VaultLimitKind, VaultRelativePath, VaultSearchLimits, VaultWalkLimits};

    static NEXT_TEST_DIRECTORY: AtomicU64 = AtomicU64::new(0);

    struct TestDirectory(PathBuf);

    impl TestDirectory {
        fn new(label: &str) -> Self {
            let sequence = NEXT_TEST_DIRECTORY.fetch_add(1, Ordering::Relaxed);
            let path = std::env::temp_dir().join(format!(
                "tcui-vault-confined-{label}-{}-{sequence}",
                std::process::id()
            ));
            fs::create_dir_all(&path).expect("create vault test directory");
            Self(path)
        }

        fn path(&self) -> &Path {
            &self.0
        }

        fn create_file(&self, relative: &str, contents: &[u8]) {
            let path = self.0.join(relative);
            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent).expect("create fixture parent");
            }
            fs::write(path, contents).expect("write fixture file");
        }

        fn open(&self) -> VaultFs {
            VaultFs::open_profile(&self.0).expect("open capability root")
        }
    }

    impl Drop for TestDirectory {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn relative(path: &str) -> VaultRelativePath {
        VaultRelativePath::parse(Path::new(path)).expect("parse test-relative path")
    }

    #[test]
    fn vault_confined_read_reads_utf8_text() {
        let _env_guard = crate::test_support::env_lock()
            .lock()
            .expect("environment lock");
        let vault = TestDirectory::new("utf8");
        vault.create_file("notes/unicode.md", "Héllo 世界".as_bytes());

        let text = vault
            .open()
            .read_text(&relative("notes/unicode.md"))
            .expect("read UTF-8 note");

        assert_eq!(text, "Héllo 世界");
    }

    #[test]
    fn vault_confined_read_returns_typed_error_for_non_utf8() {
        let _env_guard = crate::test_support::env_lock()
            .lock()
            .expect("environment lock");
        let vault = TestDirectory::new("binary");
        vault.create_file("binary.dat", &[0xff, 0xfe, 0xfd]);

        let error = vault
            .open()
            .read_text(&relative("binary.dat"))
            .expect_err("binary input must not be decoded lossily");

        assert!(matches!(error, VaultFsError::InvalidUtf8 { .. }));
    }

    #[test]
    fn vault_confined_read_rejects_outside_root_traversal_before_io() {
        let vault = TestDirectory::new("traversal");
        let sentinel = vault
            .path()
            .parent()
            .expect("temporary root has parent")
            .join("outside-sentinel.md");
        fs::write(&sentinel, b"outside secret").expect("write outside sentinel");

        let parsed = VaultRelativePath::parse(Path::new("../outside-sentinel.md"));

        let _ = fs::remove_file(sentinel);
        assert!(parsed.is_err());
    }

    #[cfg(unix)]
    #[test]
    fn vault_confined_read_never_opens_final_symlink() {
        use std::os::unix::fs::symlink;

        let _env_guard = crate::test_support::env_lock()
            .lock()
            .expect("environment lock");
        let vault = TestDirectory::new("final-symlink");
        let outside = TestDirectory::new("outside-file");
        outside.create_file("sentinel.md", b"outside secret");
        symlink(
            outside.path().join("sentinel.md"),
            vault.path().join("escape.md"),
        )
        .expect("create final symlink");

        let error = vault
            .open()
            .read_text(&relative("escape.md"))
            .expect_err("final symlink must not be opened");

        assert!(matches!(error, VaultFsError::Symlink { .. }));
    }

    #[cfg(unix)]
    #[test]
    fn vault_confined_read_never_descends_component_symlink() {
        use std::os::unix::fs::symlink;

        let _env_guard = crate::test_support::env_lock()
            .lock()
            .expect("environment lock");
        let vault = TestDirectory::new("component-symlink");
        let outside = TestDirectory::new("outside-directory");
        outside.create_file("sentinel.md", b"outside secret");
        symlink(outside.path(), vault.path().join("swapped")).expect("create component symlink");

        let error = vault
            .open()
            .read_text(&relative("swapped/sentinel.md"))
            .expect_err("component symlink must not be descended");

        assert!(matches!(error, VaultFsError::Symlink { .. }));
    }

    #[cfg(unix)]
    #[test]
    fn vault_confined_read_listing_reports_symlink_without_descending() {
        use std::os::unix::fs::symlink;

        let _env_guard = crate::test_support::env_lock()
            .lock()
            .expect("environment lock");
        let vault = TestDirectory::new("list-symlink");
        let outside = TestDirectory::new("list-outside");
        outside.create_file("sentinel.md", b"outside secret");
        symlink(outside.path(), vault.path().join("linked")).expect("create directory symlink");

        let listing = vault
            .open()
            .list(None, VaultWalkLimits::default())
            .expect("list vault");

        assert_eq!(listing.entries.len(), 1);
        assert_eq!(listing.entries[0].path, Path::new("linked"));
        assert_eq!(listing.entries[0].kind, VaultEntryKind::UnsupportedSymlink);
    }

    #[test]
    fn vault_confined_read_listing_is_deterministic_directory_first_and_hidden_filtered() {
        let _env_guard = crate::test_support::env_lock()
            .lock()
            .expect("environment lock");
        let vault = TestDirectory::new("listing-order");
        vault.create_file("zeta.md", b"z");
        vault.create_file("alpha/note.md", b"a");
        vault.create_file("beta/note.md", b"b");
        vault.create_file(".hidden.md", b"hidden");
        vault.create_file("visible/.hidden/note.md", b"hidden component");

        let fs = vault.open();
        let first = fs
            .list(None, VaultWalkLimits::default())
            .expect("first deterministic listing");
        let second = fs
            .list(None, VaultWalkLimits::default())
            .expect("second deterministic listing");

        assert_eq!(first, second);
        assert_eq!(first.entries[0].kind, VaultEntryKind::Directory);
        assert!(first.entries.iter().all(|entry| !entry
            .path
            .components()
            .any(|component| component.as_os_str().to_string_lossy().starts_with('.'))));
    }

    #[test]
    fn vault_confined_read_listing_returns_partial_metadata_at_entry_limit() {
        let _env_guard = crate::test_support::env_lock()
            .lock()
            .expect("environment lock");
        let vault = TestDirectory::new("entry-limit");
        vault.create_file("a.md", b"a");
        vault.create_file("b.md", b"b");
        vault.create_file("c.md", b"c");

        let listing = vault
            .open()
            .list(
                None,
                VaultWalkLimits {
                    max_entries: 2,
                    max_depth: 64,
                },
            )
            .expect("bounded listing");

        assert_eq!(listing.entries.len(), 2);
        assert_eq!(listing.entries[0].path, Path::new("a.md"));
        assert_eq!(listing.entries[1].path, Path::new("b.md"));
        assert!(listing.limit_reached(VaultLimitKind::Entries));
    }

    #[test]
    fn vault_confined_read_listing_returns_partial_metadata_at_depth_limit() {
        let _env_guard = crate::test_support::env_lock()
            .lock()
            .expect("environment lock");
        let vault = TestDirectory::new("depth-limit");
        vault.create_file("one/two/deep.md", b"deep");

        let listing = vault
            .open()
            .list(
                None,
                VaultWalkLimits {
                    max_entries: 50_000,
                    max_depth: 1,
                },
            )
            .expect("depth-bounded listing");

        assert!(listing.limit_reached(VaultLimitKind::Depth));
        assert!(listing
            .entries
            .iter()
            .all(|entry| entry.path.components().count() <= 1));
    }

    #[test]
    fn vault_confined_read_search_returns_partial_metadata_at_file_limit() {
        let _env_guard = crate::test_support::env_lock()
            .lock()
            .expect("environment lock");
        let vault = TestDirectory::new("search-file-limit");
        vault.create_file("a.md", b"needle");
        vault.create_file("b.md", b"needle");

        let result = vault
            .open()
            .search(
                "needle",
                VaultSearchLimits {
                    max_files: 1,
                    ..VaultSearchLimits::default()
                },
            )
            .expect("file-bounded search");

        assert_eq!(result.files_scanned, 1);
        assert!(result.limit_reached(VaultLimitKind::Files));
    }

    #[test]
    fn vault_confined_read_search_respects_total_byte_limit() {
        let _env_guard = crate::test_support::env_lock()
            .lock()
            .expect("environment lock");
        let vault = TestDirectory::new("search-total-byte-limit");
        vault.create_file("a.md", b"12345 needle");
        vault.create_file("b.md", b"needle");

        let result = vault
            .open()
            .search(
                "needle",
                VaultSearchLimits {
                    max_total_bytes: 5,
                    ..VaultSearchLimits::default()
                },
            )
            .expect("byte-bounded search");

        assert!(result.limit_reached(VaultLimitKind::TotalBytes));
        assert!(result.total_bytes_read <= 5);
    }

    #[test]
    fn vault_confined_read_search_skips_oversized_files_with_partial_metadata() {
        let _env_guard = crate::test_support::env_lock()
            .lock()
            .expect("environment lock");
        let vault = TestDirectory::new("search-file-byte-limit");
        vault.create_file("large.md", b"needle beyond bound");

        let result = vault
            .open()
            .search(
                "needle",
                VaultSearchLimits {
                    max_file_bytes: 5,
                    ..VaultSearchLimits::default()
                },
            )
            .expect("per-file-bounded search");

        assert!(result.matches.is_empty());
        assert!(result.limit_reached(VaultLimitKind::FileBytes));
    }

    #[test]
    fn vault_confined_read_search_respects_match_limit() {
        let _env_guard = crate::test_support::env_lock()
            .lock()
            .expect("environment lock");
        let vault = TestDirectory::new("search-match-limit");
        vault.create_file("many.md", b"needle needle needle");

        let result = vault
            .open()
            .search(
                "needle",
                VaultSearchLimits {
                    max_matches: 2,
                    ..VaultSearchLimits::default()
                },
            )
            .expect("match-bounded search");

        assert_eq!(result.matches.len(), 2);
        assert!(result.limit_reached(VaultLimitKind::Matches));
    }

    #[test]
    fn vault_confined_read_search_is_case_insensitive_and_hidden_filtered() {
        let _env_guard = crate::test_support::env_lock()
            .lock()
            .expect("environment lock");
        let vault = TestDirectory::new("search-hidden");
        vault.create_file("visible.md", b"Needle");
        vault.create_file(".hidden.md", b"needle");
        vault.create_file(".hidden/nested.md", b"needle");

        let result = vault
            .open()
            .search("needle", VaultSearchLimits::default())
            .expect("search visible UTF-8 files");

        assert_eq!(result.matches.len(), 1);
        assert_eq!(result.matches[0].path, Path::new("visible.md"));
    }
}
