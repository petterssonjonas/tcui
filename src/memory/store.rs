use std::collections::HashSet;
use std::path::{Path, PathBuf};

use serde::Serialize;
use thiserror::Error;

use super::embedding::{as_blob, embed, EmbeddingError, DIMENSIONS, MODEL_ID};
use super::index::{IndexError, MemoryIndex};
use super::paths::{MemoryPaths, PathError};
use super::sync::synchronize;

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
}

impl MemoryStore {
    pub(crate) fn open(vault: &Path, database: &Path) -> Result<Self, MemoryError> {
        let paths = MemoryPaths::new(vault)?;
        let _ = MemoryIndex::open(database)?;
        Ok(Self {
            paths,
            database: database.to_path_buf(),
        })
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
            "SELECT f.rel_path, f.title, c.start_byte, c.end_byte, v.distance
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
                    row.get::<_, String>(1)?,
                    row.get::<_, i64>(2)?,
                    row.get::<_, i64>(3)?,
                    row.get::<_, f32>(4)?,
                ))
            },
        )?;
        let mut seen = HashSet::new();
        let mut hits = Vec::new();
        for row in rows {
            let (relative, title, start, end, distance) = row?;
            if !seen.insert(relative.clone()) {
                continue;
            }
            let target = self.paths.existing_target(Path::new(&relative))?;
            let content = std::fs::read_to_string(&target.absolute)?;
            let start = usize::try_from(start).unwrap_or(usize::MAX);
            let end = usize::try_from(end).unwrap_or(usize::MAX);
            let excerpt = content
                .get(start..end)
                .unwrap_or_default()
                .trim()
                .chars()
                .take(600)
                .collect();
            hits.push(MemoryHit {
                path: target.relative,
                title,
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
        Ok(std::fs::read_to_string(
            self.paths.existing_target(path)?.absolute,
        )?)
    }

    pub(crate) fn list_files(&self) -> Result<Vec<(PathBuf, String)>, MemoryError> {
        self.sync()?;
        let index = MemoryIndex::open(&self.database)?;
        let mut statement = index
            .conn
            .prepare("SELECT rel_path, title FROM memory_files ORDER BY rel_path")?;
        let rows = statement.query_map([], |row| {
            Ok((
                PathBuf::from(row.get::<_, String>(0)?),
                row.get::<_, String>(1)?,
            ))
        })?;
        Ok(rows.collect::<Result<Vec<_>, _>>()?)
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
            vault: self
                .paths
                .root()
                .parent()
                .unwrap_or(self.paths.root())
                .to_path_buf(),
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
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::MemoryStore;
    use crate::memory::WriteOutcome;

    fn fixture() -> (std::path::PathBuf, std::path::PathBuf) {
        let root =
            std::env::temp_dir().join(format!("tcui-memory-store-{}", rand::random::<u64>()));
        let cache = root.join("cache.sqlite3");
        fs::create_dir_all(root.join("memories")).expect("memory fixture");
        (root, cache)
    }

    #[test]
    fn sync_detects_create_edit_rename_and_delete() {
        // Given
        let (vault, cache) = fixture();
        fs::write(
            vault.join("memories/editor.md"),
            "# Editor\n\nUse Neovim.\n",
        )
        .expect("note");
        let store = MemoryStore::open(&vault, &cache).expect("memory store");

        // When / Then: create
        store.sync().expect("initial sync");
        assert_eq!(store.status().expect("status").files, 1);
        assert_eq!(
            store.search("Neovim editor", 8).expect("search")[0].title,
            "Editor"
        );

        // When / Then: edit
        fs::write(vault.join("memories/editor.md"), "# Editor\n\nUse Helix.\n").expect("edit");
        store.sync().expect("edit sync");
        assert!(store
            .search("Helix editor", 8)
            .expect("edited search")
            .iter()
            .any(|hit| hit.excerpt.contains("Helix")));

        // When / Then: rename
        fs::rename(
            vault.join("memories/editor.md"),
            vault.join("memories/preferred-editor.md"),
        )
        .expect("rename");
        store.sync().expect("rename sync");
        assert_eq!(store.status().expect("renamed status").files, 1);

        // When / Then: delete
        fs::remove_file(vault.join("memories/preferred-editor.md")).expect("delete");
        store.sync().expect("delete sync");
        assert_eq!(store.status().expect("deleted status").files, 0);
        fs::remove_dir_all(vault).expect("fixture cleanup");
    }

    #[test]
    fn remember_deduplicates_and_forget_moves_to_trash() {
        // Given
        let (vault, cache) = fixture();
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
        assert!(trashed.starts_with(vault.join(".trash/tcui-memory")));
        assert!(trashed.exists());
        fs::remove_dir_all(vault).expect("fixture cleanup");
    }
}
