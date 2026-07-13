use std::collections::HashSet;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use thiserror::Error;

use super::embedding::{DIMENSIONS, EmbeddingError, MODEL_ID, as_blob, embed};
use super::index::{IndexError, MemoryIndex};
use super::paths::{MemoryPaths, PathError};
use super::sync::synchronize;
use crate::storage::crypto::{
    SharedKey, StorageCryptoError, read_encrypted_document, write_encrypted_document,
};

#[derive(Debug, Error)]
pub(crate) enum MemoryError {
    #[error("memory I/O failed: {0}")]
    Io(#[from] std::io::Error),
    #[error("memory database failed: {0}")]
    Sql(#[from] rusqlite::Error),
    #[error("memory cache failed: {0}")]
    Index(#[from] IndexError),
    #[error("memory model failed: {0}")]
    Embedding(#[from] EmbeddingError),
    #[error("invalid memory path: {0}")]
    Path(#[from] PathError),
    #[error("invalid memory request: {0}")]
    Invalid(String),
    #[error("memory scan failed: {0}")]
    Walk(String),
    #[error("memory serialization failed: {0}")]
    Json(#[from] serde_json::Error),
    #[error("memory encryption failed: {0}")]
    Crypto(#[from] StorageCryptoError),
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub(crate) struct MemoryHit {
    pub(crate) path: PathBuf,
    pub(crate) title: String,
    pub(crate) similarity: f32,
    pub(crate) excerpt: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(crate) struct MemoryStatus {
    pub(crate) vault: PathBuf,
    pub(crate) cache: PathBuf,
    pub(crate) model_id: &'static str,
    pub(crate) dimensions: usize,
    pub(crate) files: usize,
    pub(crate) chunks: usize,
}

#[derive(Debug, Clone)]
pub(crate) struct MemoryStore {
    pub(super) paths: MemoryPaths,
    pub(super) database: PathBuf,
    pub(super) key: SharedKey,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct MemoryDocument {
    pub(crate) schema_version: u32,
    pub(crate) id: u64,
    pub(crate) logical_path: PathBuf,
    pub(crate) title: String,
    pub(crate) created_at_ms: i64,
    pub(crate) updated_at_ms: i64,
    pub(crate) markdown: String,
}

pub(crate) const MEMORY_DOCUMENT_KIND: &str = "memory";
const MEMORY_DOCUMENT_SCHEMA_VERSION: u32 = 1;

impl MemoryStore {
    pub(crate) fn open(vault: &Path, database: &Path) -> Result<Self, MemoryError> {
        let key =
            SharedKey::load_or_create_default(&crate::storage::paths::TcuiDataPaths::discover())?
                .key;
        Self::open_with_key(vault, database, key)
    }

    pub(crate) fn open_with_key(
        vault: &Path,
        database: &Path,
        key: SharedKey,
    ) -> Result<Self, MemoryError> {
        let paths = MemoryPaths::new(vault)?;
        let store = Self {
            paths,
            database: database.to_path_buf(),
            key,
        };
        store.archive_legacy_plaintext_memories()?;
        let _ = MemoryIndex::open(database)?;
        Ok(store)
    }

    pub(crate) fn default_cache_path() -> PathBuf {
        dirs::cache_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("tcui/memory.sqlite3")
    }

    pub(crate) fn sync(&self) -> Result<(), MemoryError> {
        synchronize(&self.paths, &self.database)
    }

    pub(crate) fn search(&self, query: &str, limit: usize) -> Result<Vec<MemoryHit>, MemoryError> {
        self.sync()?;
        let embedding = as_blob(&embed(query)?);
        let index = MemoryIndex::open(&self.database)?;
        let candidate_limit = limit.clamp(1, 100).saturating_mul(4);
        let mut statement = index.conn.prepare(
            "SELECT f.rel_path, c.start_byte, c.end_byte, v.distance
             FROM vec_memory_chunks v
             JOIN memory_chunks c ON c.id = v.chunk_id
             JOIN memory_files f ON f.id = c.file_id
             WHERE v.embedding MATCH ?1 AND k = ?2
             ORDER BY v.distance",
        )?;
        let rows = statement.query_map(
            rusqlite::params![
                embedding,
                i64::try_from(candidate_limit).unwrap_or(i64::MAX)
            ],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, i64>(1)?,
                    row.get::<_, i64>(2)?,
                    row.get::<_, f32>(3)?,
                ))
            },
        )?;
        let mut seen = HashSet::new();
        let mut hits = Vec::new();
        for row in rows {
            let (relative, start, end, distance) = row?;
            if !seen.insert(relative.clone()) {
                continue;
            }
            let Some(document) = self.find_document_by_physical_name(&relative)? else {
                continue;
            };
            let start = usize::try_from(start).unwrap_or(usize::MAX);
            let end = usize::try_from(end).unwrap_or(usize::MAX);
            let excerpt = document
                .markdown
                .get(start..end)
                .unwrap_or_default()
                .trim()
                .chars()
                .take(600)
                .collect();
            hits.push(MemoryHit {
                path: document.logical_path,
                title: document.title,
                similarity: 1.0 - distance,
                excerpt,
            });
            if hits.len() >= limit {
                break;
            }
        }
        Ok(hits)
    }

    pub(crate) fn read(&self, path: &Path) -> Result<String, MemoryError> {
        self.sync()?;
        let logical = self.paths.logical_path(path)?;
        let document = self
            .find_document_by_logical_path(&logical)?
            .ok_or_else(|| MemoryError::Invalid("memory path is unavailable".to_string()))?;
        Ok(document.markdown)
    }

    pub(crate) fn list_files(&self) -> Result<Vec<(PathBuf, String)>, MemoryError> {
        self.sync()?;
        let mut files = self
            .active_documents()?
            .into_iter()
            .map(|(_, document)| (document.logical_path, document.title))
            .collect::<Vec<_>>();
        files.sort_by(|left, right| left.0.cmp(&right.0));
        Ok(files)
    }

    pub(crate) fn status(&self) -> Result<MemoryStatus, MemoryError> {
        self.sync()?;
        let index = MemoryIndex::open(&self.database)?;
        let files = index
            .conn
            .query_row("SELECT COUNT(*) FROM memory_files", [], |row| {
                row.get::<_, i64>(0)
            })?;
        let chunks = index
            .conn
            .query_row("SELECT COUNT(*) FROM memory_chunks", [], |row| {
                row.get::<_, i64>(0)
            })?;
        Ok(MemoryStatus {
            vault: self.paths.vault().to_path_buf(),
            cache: self.database.clone(),
            model_id: MODEL_ID,
            dimensions: DIMENSIONS,
            files: usize::try_from(files).unwrap_or(usize::MAX),
            chunks: usize::try_from(chunks).unwrap_or(usize::MAX),
        })
    }

    pub(crate) fn reindex(&self) -> Result<MemoryStatus, MemoryError> {
        if self.database.exists() {
            std::fs::remove_file(&self.database)?;
        }
        self.status()
    }

    pub(crate) fn active_documents(&self) -> Result<Vec<(PathBuf, MemoryDocument)>, MemoryError> {
        let mut documents = Vec::new();
        if !self.paths.root().exists() {
            return Ok(documents);
        }

        for entry in std::fs::read_dir(self.paths.root())? {
            let entry = entry?;
            let path = entry.path();
            if !is_memory_document_path(&path) {
                continue;
            }
            let document = self.read_document_at(&path)?;
            documents.push((path, document));
        }
        documents.sort_by(|left, right| left.1.logical_path.cmp(&right.1.logical_path));
        Ok(documents)
    }

    pub(crate) fn find_document_by_logical_path(
        &self,
        logical_path: &Path,
    ) -> Result<Option<MemoryDocument>, MemoryError> {
        let logical_path = self.paths.logical_path(logical_path)?;
        Ok(self
            .active_documents()?
            .into_iter()
            .find_map(|(_, document)| (document.logical_path == logical_path).then_some(document)))
    }

    pub(crate) fn physical_path_for_logical_path(
        &self,
        logical_path: &Path,
    ) -> Result<Option<PathBuf>, MemoryError> {
        let logical_path = self.paths.logical_path(logical_path)?;
        Ok(self
            .active_documents()?
            .into_iter()
            .find_map(|(path, document)| (document.logical_path == logical_path).then_some(path)))
    }

    pub(crate) fn find_document_by_physical_name(
        &self,
        physical_name: &str,
    ) -> Result<Option<MemoryDocument>, MemoryError> {
        Ok(self
            .active_documents()?
            .into_iter()
            .find_map(|(path, document)| {
                path.file_name()
                    .and_then(|value| value.to_str())
                    .is_some_and(|value| value == physical_name)
                    .then_some(document)
            }))
    }

    pub(crate) fn read_document_at(&self, path: &Path) -> Result<MemoryDocument, MemoryError> {
        Ok(read_encrypted_document(
            path,
            &self.key,
            MEMORY_DOCUMENT_KIND,
        )?)
    }

    pub(crate) fn write_document_at(
        &self,
        path: &Path,
        document: &MemoryDocument,
    ) -> Result<(), MemoryError> {
        Ok(write_encrypted_document(
            path,
            &self.key,
            MEMORY_DOCUMENT_KIND,
            document,
        )?)
    }

    pub(crate) fn allocate_document_id(&self) -> u64 {
        loop {
            let id = rand::random::<u64>();
            if !self.paths.active_document_path(id).exists()
                && !self.paths.trash_document_path(id).exists()
            {
                return id;
            }
        }
    }

    pub(crate) fn archive_legacy_plaintext_memories(&self) -> Result<(), MemoryError> {
        for entry in walkdir::WalkDir::new(self.paths.legacy_root()).follow_links(false) {
            let entry = entry.map_err(|error| MemoryError::Walk(error.to_string()))?;
            if entry.file_type().is_symlink()
                || !entry.file_type().is_file()
                || entry.path().extension().and_then(|value| value.to_str()) != Some("md")
            {
                continue;
            }
            let relative = entry
                .path()
                .strip_prefix(self.paths.legacy_root())
                .map_err(|_| MemoryError::Invalid("memory path escaped its root".to_string()))?;
            let target = self.paths.legacy_target(relative)?;
            let markdown = std::fs::read_to_string(&target.absolute)?;
            let fallback = target
                .relative
                .file_stem()
                .and_then(|value| value.to_str())
                .unwrap_or("Memory");
            let parsed = super::markdown::parse_memory(&markdown, fallback);
            let timestamp = now_ms();
            let document = MemoryDocument {
                schema_version: MEMORY_DOCUMENT_SCHEMA_VERSION,
                id: self.allocate_document_id(),
                logical_path: target.relative.clone(),
                title: parsed.title,
                created_at_ms: timestamp,
                updated_at_ms: timestamp,
                markdown: normalize_markdown(&markdown),
            };
            let destination = self.paths.trash_document_path(document.id);
            if let Err(error) = self.write_document_at(&destination, &document) {
                eprintln!("warning: failed to archive legacy memory");
                let _ = error;
                continue;
            }
            match self.read_document_at(&destination) {
                Ok(verified)
                    if verified.logical_path == document.logical_path
                        && verified.markdown == document.markdown =>
                {
                    let _ = std::fs::remove_file(&target.absolute);
                }
                Ok(_) | Err(_) => {
                    let _ = std::fs::remove_file(&destination);
                    eprintln!("warning: failed to verify archived legacy memory");
                }
            }
        }
        Ok(())
    }
}

pub(crate) fn normalize_markdown(markdown: &str) -> String {
    let trimmed = markdown.replace("\r\n", "\n").replace('\r', "\n");
    if trimmed.ends_with('\n') {
        trimmed
    } else {
        format!("{trimmed}\n")
    }
}

pub(crate) fn now_ms() -> i64 {
    static LAST_TIMESTAMP_MS: std::sync::atomic::AtomicI64 = std::sync::atomic::AtomicI64::new(0);

    let observed = i64::try_from(
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("system clock before epoch")
            .as_millis(),
    )
    .unwrap_or(i64::MAX);
    let mut previous = LAST_TIMESTAMP_MS.load(std::sync::atomic::Ordering::Relaxed);

    loop {
        let timestamp = observed.max(previous.saturating_add(1));
        match LAST_TIMESTAMP_MS.compare_exchange(
            previous,
            timestamp,
            std::sync::atomic::Ordering::SeqCst,
            std::sync::atomic::Ordering::SeqCst,
        ) {
            Ok(_) => return timestamp,
            Err(current) => previous = current,
        }
    }
}

pub(crate) fn is_memory_document_path(path: &Path) -> bool {
    path.is_file()
        && path
            .extension()
            .and_then(|value| value.to_str())
            .is_some_and(|value| value == "tcui-memory")
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::{Path, PathBuf};

    use super::{MemoryDocument, MemoryStore};
    use crate::memory::WriteOutcome;
    use crate::storage::crypto::{SharedKey, read_encrypted_document};
    use crate::storage::paths::TcuiDataPaths;

    fn env_lock() -> &'static std::sync::Mutex<()> {
        crate::test_support::env_lock()
    }

    fn fixture() -> (PathBuf, PathBuf, PathBuf) {
        let root =
            std::env::temp_dir().join(format!("tcui-memory-store-{}", rand::random::<u64>()));
        let cache = root.join("cache.sqlite3");
        fs::create_dir_all(root.join("memories")).expect("memory fixture");
        let data_home = root.join("data-home");
        fs::create_dir_all(&data_home).expect("data home");
        (root, cache, data_home)
    }

    #[test]
    fn sync_detects_create_edit_rename_and_delete() {
        // Given
        let _guard = env_lock().lock().expect("env lock poisoned");
        let (vault, cache, data_home) = fixture();
        std::env::set_var("XDG_DATA_HOME", &data_home);
        let store = MemoryStore::open(&vault, &cache).expect("memory store");
        let WriteOutcome::Saved { path, .. } = store
            .write(Path::new("editor.md"), "# Editor\n\nUse Neovim.\n", false)
            .expect("save note")
        else {
            panic!("memory should be saved");
        };

        // When / Then: create
        store.sync().expect("initial sync");
        assert_eq!(store.status().expect("status").files, 1);
        assert_eq!(
            store.list_files().expect("list files"),
            vec![(PathBuf::from("editor.md"), "Editor".to_string())]
        );

        // When / Then: edit
        store
            .write(Path::new(&path), "# Editor\n\nUse Helix.\n", true)
            .expect("edit");
        store.sync().expect("edit sync");
        assert_eq!(
            store.read(Path::new(&path)).expect("edited read"),
            "# Editor\n\nUse Helix.\n"
        );

        // When / Then: rename
        let markdown = store.read(Path::new(&path)).expect("read memory");
        store
            .write(Path::new("preferred-editor.md"), &markdown, false)
            .expect("renamed memory");
        store.forget(Path::new(&path)).expect("forget original");
        store.sync().expect("rename sync");
        assert_eq!(store.status().expect("renamed status").files, 1);
        assert_eq!(
            store.list_files().expect("renamed list"),
            vec![(PathBuf::from("preferred-editor.md"), "Editor".to_string())]
        );

        // When / Then: delete
        store
            .forget(Path::new("preferred-editor.md"))
            .expect("delete");
        store.sync().expect("delete sync");
        assert_eq!(store.status().expect("deleted status").files, 0);
        fs::remove_dir_all(vault).expect("fixture cleanup");
        std::env::remove_var("XDG_DATA_HOME");
    }

    #[test]
    fn remember_deduplicates_and_forget_moves_to_trash() {
        // Given
        let _guard = env_lock().lock().expect("env lock poisoned");
        let (vault, cache, data_home) = fixture();
        std::env::set_var("XDG_DATA_HOME", &data_home);
        let store = MemoryStore::open(&vault, &cache).expect("memory store");

        // When
        let first = store
            .remember("User prefers concise explanations.")
            .expect("first memory");
        let second = store
            .remember("User prefers concise explanations.")
            .expect("duplicate memory");

        // Then
        let WriteOutcome::Saved { path, .. } = first else {
            panic!("first memory should be saved");
        };
        assert!(matches!(second, WriteOutcome::AlreadyKnown { .. }));
        let trashed = store
            .forget(std::path::Path::new(&path))
            .expect("forget memory");
        assert!(trashed.starts_with(data_home.join("tcui").join("memories").join(".trash")));
        assert!(trashed.exists());
        fs::remove_dir_all(vault).expect("fixture cleanup");
        std::env::remove_var("XDG_DATA_HOME");
    }

    #[test]
    fn active_memories_are_encrypted_and_read_by_logical_path() {
        let _guard = env_lock().lock().expect("env lock poisoned");
        let (vault, cache, data_home) = fixture();
        std::env::set_var("XDG_DATA_HOME", &data_home);
        let store = MemoryStore::open(&vault, &cache).expect("memory store");

        let WriteOutcome::Saved { path, .. } = store
            .write(
                Path::new("preferences/editor.md"),
                "# Editor\n\nUse Helix.\n",
                false,
            )
            .expect("save memory")
        else {
            panic!("memory should be saved");
        };

        let files = store.active_documents().expect("active documents");
        assert_eq!(files.len(), 1);
        let raw = std::fs::read_to_string(&files[0].0).expect("read raw memory file");
        assert!(raw.starts_with("enc:v1:"));
        assert!(!raw.contains("Use Helix."));
        assert_eq!(
            store.read(Path::new(&path)).expect("read logical path"),
            "# Editor\n\nUse Helix.\n"
        );

        fs::remove_dir_all(vault).expect("fixture cleanup");
        std::env::remove_var("XDG_DATA_HOME");
    }

    #[test]
    fn legacy_vault_memories_are_archived_to_encrypted_trash() {
        let _guard = env_lock().lock().expect("env lock poisoned");
        let (vault, cache, data_home) = fixture();
        std::env::set_var("XDG_DATA_HOME", &data_home);
        fs::create_dir_all(vault.join("memories/preferences")).expect("legacy dirs");
        fs::write(
            vault.join("memories/preferences/editor.md"),
            "---\ntitle: Preferred editor\n---\n\nUse Neovim.\n",
        )
        .expect("legacy note");

        let store = MemoryStore::open(&vault, &cache).expect("memory store");
        assert!(store.list_files().expect("active list").is_empty());
        assert!(
            !vault.join("memories/preferences/editor.md").exists(),
            "legacy plaintext should be removed after archival"
        );

        let trash_root = data_home.join("tcui").join("memories").join(".trash");
        let archived = std::fs::read_dir(&trash_root)
            .expect("trash dir")
            .map(|entry| entry.expect("trash entry").path())
            .find(|path| path.extension().and_then(|value| value.to_str()) == Some("tcui-memory"))
            .expect("archived encrypted memory");
        let key = SharedKey::load_or_create_default(&TcuiDataPaths::discover())
            .expect("shared key")
            .key;
        let document: MemoryDocument =
            read_encrypted_document(&archived, &key, "memory").expect("read archived memory");
        assert_eq!(
            document.logical_path,
            PathBuf::from("preferences/editor.md")
        );
        assert_eq!(document.title, "Preferred editor");
        assert!(document.markdown.contains("Use Neovim."));

        fs::remove_dir_all(vault).expect("fixture cleanup");
        std::env::remove_var("XDG_DATA_HOME");
    }

    #[test]
    fn add_from_plaintext_imports_markdown_without_mutating_source() {
        let _guard = env_lock().lock().expect("env lock poisoned");
        let (vault, cache, data_home) = fixture();
        std::env::set_var("XDG_DATA_HOME", &data_home);
        let import_root = vault.join("imports");
        fs::create_dir_all(&import_root).expect("import dir");
        let source = import_root.join("snippet.txt");
        fs::write(&source, "Plain text fact").expect("source note");
        let original = fs::read_to_string(&source).expect("read source");
        let store = MemoryStore::open(&vault, &cache).expect("memory store");

        let outcome = store.add_from_plaintext(&source).expect("import source");
        let WriteOutcome::Saved { path, .. } = outcome else {
            panic!("memory should be saved");
        };
        assert_eq!(path, "snippet.md");
        assert_eq!(
            fs::read_to_string(&source).expect("source preserved"),
            original
        );
        assert_eq!(
            store.read(Path::new("snippet.md")).expect("imported read"),
            "Plain text fact\n"
        );

        fs::remove_dir_all(vault).expect("fixture cleanup");
        std::env::remove_var("XDG_DATA_HOME");
    }

    #[test]
    fn sqlite_cache_avoids_plaintext_memory_metadata_and_content() {
        let _guard = env_lock().lock().expect("env lock poisoned");
        let (vault, cache, data_home) = fixture();
        std::env::set_var("XDG_DATA_HOME", &data_home);
        let store = MemoryStore::open(&vault, &cache).expect("memory store");
        store
            .write(
                Path::new("preferences/editor.md"),
                "---\ntitle: Preferred editor\n---\n\nUse Helix.\n",
                false,
            )
            .expect("save memory");
        store.sync().expect("sync memory");

        let connection = rusqlite::Connection::open(&cache).expect("open cache");
        let rel_paths = connection
            .prepare("SELECT rel_path FROM memory_files")
            .expect("prepare rel_path query")
            .query_map([], |row| row.get::<_, String>(0))
            .expect("query rel_paths")
            .collect::<rusqlite::Result<Vec<_>>>()
            .expect("collect rel_paths");
        let combined_text = rel_paths.join("\n");
        assert!(!combined_text.contains("preferences/editor.md"));
        assert!(!combined_text.contains("Preferred editor"));
        assert!(!combined_text.contains("Use Helix."));

        fs::remove_dir_all(vault).expect("fixture cleanup");
        std::env::remove_var("XDG_DATA_HOME");
    }

    #[test]
    fn deleting_cache_rebuilds_search_from_encrypted_source_files() {
        let _guard = env_lock().lock().expect("env lock poisoned");
        let (vault, cache, data_home) = fixture();
        std::env::set_var("XDG_DATA_HOME", &data_home);
        let store = MemoryStore::open(&vault, &cache).expect("memory store");
        store
            .write(
                Path::new("preferences/editor.md"),
                "# Editor\n\nUse Helix.\n",
                false,
            )
            .expect("save memory");
        store.sync().expect("sync memory");
        let before = store.list_files().expect("list before rebuild");

        std::fs::remove_file(&cache).expect("remove cache");
        let after = store.reindex().expect("reindex after cache removal");
        assert_eq!(after.files, 1);
        assert_eq!(store.list_files().expect("list after rebuild"), before);

        fs::remove_dir_all(vault).expect("fixture cleanup");
        std::env::remove_var("XDG_DATA_HOME");
    }
}
