use std::io;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
pub(crate) struct TcuiDataPaths {
    pub(crate) root: PathBuf,
    pub(crate) database: PathBuf,
    pub(crate) chat_key: PathBuf,
    pub(crate) keys_file: PathBuf,
    pub(crate) chats_dir: PathBuf,
    pub(crate) chats_trash_dir: PathBuf,
    pub(crate) memories_dir: PathBuf,
    pub(crate) memories_trash_dir: PathBuf,
}

impl TcuiDataPaths {
    pub(crate) fn discover() -> Self {
        let root = dirs::data_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("tcui");
        Self::from_root(root)
    }

    pub(crate) fn from_root(root: PathBuf) -> Self {
        Self {
            database: root.join("tcui.db"),
            chat_key: root.join("chat.key"),
            keys_file: root.join("keys.toml"),
            chats_dir: root.join("chats"),
            chats_trash_dir: root.join("chats").join(".trash"),
            memories_dir: root.join("memories"),
            memories_trash_dir: root.join("memories").join(".trash"),
            root,
        }
    }

    pub(crate) fn ensure_layout(&self) -> io::Result<()> {
        ensure_directory(&self.root)?;
        ensure_directory(&self.chats_dir)?;
        ensure_directory(&self.chats_trash_dir)?;
        ensure_directory(&self.memories_dir)?;
        ensure_directory(&self.memories_trash_dir)?;
        Ok(())
    }
}

pub(crate) fn ensure_directory(path: &Path) -> io::Result<()> {
    std::fs::create_dir_all(path)?;
    set_unix_mode(path, 0o700)?;
    Ok(())
}

pub(crate) fn directory_has_entries(path: &Path) -> io::Result<bool> {
    if !path.exists() {
        return Ok(false);
    }

    let mut entries = std::fs::read_dir(path)?;
    Ok(entries.next().transpose()?.is_some())
}

pub(crate) fn directory_has_non_trash_entries(path: &Path) -> io::Result<bool> {
    if !path.exists() {
        return Ok(false);
    }

    for entry in std::fs::read_dir(path)? {
        let entry = entry?;
        if entry.file_name() != ".trash" {
            return Ok(true);
        }
    }
    Ok(false)
}

pub(crate) fn set_unix_mode(path: &Path, mode: u32) -> io::Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;

        let mut permissions = std::fs::metadata(path)?.permissions();
        permissions.set_mode(mode);
        std::fs::set_permissions(path, permissions)?;
    }

    #[cfg(not(unix))]
    let _ = (path, mode);

    Ok(())
}
