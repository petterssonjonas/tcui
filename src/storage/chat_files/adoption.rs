use std::io;
use std::path::{Path, PathBuf};

use cap_fs_ext::DirExt;

use super::{ChatFileError, ChatSavedFileStore};

impl ChatSavedFileStore {
    /// Check native metadata and every stored body, including Trash, without decoding data.
    /// This method takes no locks. Mutation callers must already hold VaultMutationGuard.
    pub(crate) fn require_native_adoption(&self) -> Result<(), ChatFileError> {
        self.require_auxiliary_adoption()?;
        self.scan_native_files(None)
    }

    pub(super) fn require_auxiliary_adoption(&self) -> Result<(), ChatFileError> {
        scan_tree(&self.paths.chat_file_links_dir, None, true)?;
        for directory in ["vault-backups", "vault-native-ops"] {
            scan_tree(&self.paths.root.join(directory), None, true)?;
        }
        Ok(())
    }

    pub(super) fn scan_native_files(
        &self,
        paths: Option<&mut Vec<PathBuf>>,
    ) -> Result<(), ChatFileError> {
        crate::vault::require_root(&self.saved_root)?;
        scan_tree(&self.saved_root, paths, false)
    }
}

pub(super) fn require_path_adoption(path: &Path) -> Result<(), ChatFileError> {
    let prefix = match crate::protection::read_regular_prefix(
        path,
        crate::protection::NATIVE_STORE_ADOPTION_PROBE_LEN,
    ) {
        Ok(prefix) => prefix,
        Err(error)
            if error
                .downcast_ref::<io::Error>()
                .is_some_and(|error| error.kind() == io::ErrorKind::NotFound) =>
        {
            return Ok(())
        }
        Err(error) => return Err(ChatFileError::from_protection(error)),
    };
    crate::protection::require_native_store_adoption(crate::protection::Domain::Vault, &prefix)
        .map_err(ChatFileError::from_protection)
}

fn scan_tree(
    root: &Path,
    mut paths: Option<&mut Vec<PathBuf>>,
    skip_locks: bool,
) -> Result<(), ChatFileError> {
    let directory = match crate::protection::open_directory(root, false) {
        Ok(directory) => directory,
        Err(error)
            if error
                .downcast_ref::<io::Error>()
                .is_some_and(|error| error.kind() == io::ErrorKind::NotFound) =>
        {
            return Ok(())
        }
        Err(error) => return Err(ChatFileError::from_protection(error)),
    };
    let mut pending = vec![(directory, PathBuf::new())];
    while let Some((directory, relative)) = pending.pop() {
        for entry in directory.entries()? {
            let entry = entry?;
            let name = entry.file_name();
            let bytes = name.as_encoded_bytes();
            // Recovery owns stages and atomic temporaries; they are not authoritative files.
            if (bytes.starts_with(b".tcui-") && bytes.ends_with(b".stage"))
                || (bytes.starts_with(b".chat-file-") && bytes.ends_with(b".tmp"))
                || (skip_locks && (bytes.ends_with(b".lock") || bytes == b"lock"))
            {
                continue;
            }
            let path = relative.join(&name);
            let metadata = directory.symlink_metadata(&name)?;
            if metadata.is_dir() {
                pending.push((directory.open_dir_nofollow(&name)?, path));
            } else if metadata.is_file() {
                require_path_adoption(&root.join(&path))?;
                if let Some(paths) = paths.as_deref_mut() {
                    if super::items::item_path_is_valid(&path) {
                        paths.push(path);
                    }
                }
            }
        }
    }
    Ok(())
}
