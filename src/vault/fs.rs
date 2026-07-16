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

pub struct VaultFs(Dir);

impl VaultFs {
    pub fn open_profile(root: &Path) -> Result<Self, VaultFsError> {
        Dir::open_ambient_dir(root, cap_std::ambient_authority())
            .map(Self)
            .map_err(|source| VaultFsError::OpenProfile {
                path: root.to_path_buf(),
                source,
            })
    }

    pub fn read_text(&self, path: &VaultRelativePath) -> Result<String, VaultFsError> {
        let path = path.as_path();
        let mut file = self.open_file_nofollow(path)?;
        let mut bytes = Vec::new();
        file.read_to_end(&mut bytes)
            .map_err(|source| io_error("read", path, source))?;
        String::from_utf8(bytes).map_err(|source| VaultFsError::InvalidUtf8 {
            path: path.to_path_buf(),
            source,
        })
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
        super::editor::validate_editor_target(canonical_root, &self.0, path)
    }

    pub fn list(
        &self,
        start: Option<&VaultRelativePath>,
        limits: VaultWalkLimits,
    ) -> Result<VaultListing, VaultFsError> {
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
            let mut file = self.open_file_nofollow(&entry.path)?;
            let metadata = file
                .metadata()
                .map_err(|source| io_error("read metadata for", &entry.path, source))?;
            if metadata.len() > limits.max_file_bytes {
                record_limit(
                    &mut result.limits,
                    VaultLimitKind::FileBytes,
                    Some(entry.path),
                );
                continue;
            }
            let remaining = limits
                .max_total_bytes
                .saturating_sub(result.total_bytes_read);
            if metadata.len() > remaining {
                record_limit(
                    &mut result.limits,
                    VaultLimitKind::TotalBytes,
                    Some(entry.path),
                );
                break;
            }

            let mut bytes =
                Vec::with_capacity(usize::try_from(metadata.len()).unwrap_or(1024 * 1024));
            file.by_ref()
                .take(remaining)
                .read_to_end(&mut bytes)
                .map_err(|source| io_error("search", &entry.path, source))?;
            result.total_bytes_read += u64::try_from(bytes.len()).unwrap_or(u64::MAX);

            let final_size = file
                .metadata()
                .map_err(|source| io_error("recheck metadata for", &entry.path, source))?
                .len();
            if final_size > limits.max_file_bytes {
                record_limit(
                    &mut result.limits,
                    VaultLimitKind::FileBytes,
                    Some(entry.path),
                );
                continue;
            }
            if final_size > u64::try_from(bytes.len()).unwrap_or(u64::MAX) {
                record_limit(
                    &mut result.limits,
                    VaultLimitKind::TotalBytes,
                    Some(entry.path),
                );
                break;
            }

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
