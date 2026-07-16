use std::io;
use std::path::PathBuf;

use super::paths::ensure_directory;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct TcuiCachePaths {
    pub(crate) root: PathBuf,
    pub(crate) chat_files_dir: PathBuf,
}

impl TcuiCachePaths {
    pub(crate) fn discover() -> Self {
        let root = dirs::cache_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("tcui");
        Self::from_root(root)
    }

    pub(crate) fn from_root(root: PathBuf) -> Self {
        Self {
            chat_files_dir: root.join("chat-files"),
            root,
        }
    }

    pub(crate) fn ensure_layout(&self) -> io::Result<()> {
        ensure_directory(&self.root)?;
        ensure_directory(&self.chat_files_dir)
    }

    pub(crate) fn conversation_dir(&self, conversation_id: i64) -> PathBuf {
        self.chat_files_dir.join(conversation_id.to_string())
    }
}
