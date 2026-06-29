use std::path::Path;
use std::sync::OnceLock;

use rusqlite::{ffi::sqlite3_auto_extension, params, Connection};
use thiserror::Error;

use super::embedding::{DIMENSIONS, MODEL_ID};

const SCHEMA_VERSION: &str = "1";
static SQLITE_VEC_REGISTRATION: OnceLock<i32> = OnceLock::new();

#[derive(Debug, Error)]
pub(crate) enum IndexError {
    #[error("memory cache I/O failed: {0}")]
    Io(#[from] std::io::Error),
    #[error("memory cache database failed: {0}")]
    Sql(#[from] rusqlite::Error),
    #[error("sqlite-vec registration failed with code {0}")]
    Registration(i32),
}

pub(crate) struct MemoryIndex {
    pub(super) conn: Connection,
}

impl MemoryIndex {
    pub(crate) fn open(path: &Path) -> Result<Self, IndexError> {
        register_sqlite_vec()?;
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let conn = Connection::open(path)?;
        conn.busy_timeout(std::time::Duration::from_secs(5))?;
        conn.execute_batch("PRAGMA foreign_keys = ON; PRAGMA journal_mode = WAL;")?;
        let mut index = Self { conn };
        index.ensure_schema()?;
        Ok(index)
    }

    pub(crate) fn meta(&self, key: &str) -> Result<Option<String>, IndexError> {
        let mut statement = self
            .conn
            .prepare("SELECT value FROM memory_meta WHERE key = ?1")?;
        let mut rows = statement.query([key])?;
        Ok(rows.next()?.map(|row| row.get(0)).transpose()?)
    }

    #[cfg(test)]
    pub(crate) fn set_meta_for_test(&self, key: &str, value: &str) -> Result<(), IndexError> {
        self.conn.execute(
            "INSERT INTO memory_meta(key, value) VALUES (?1, ?2)
             ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            params![key, value],
        )?;
        Ok(())
    }

    fn ensure_schema(&mut self) -> Result<(), IndexError> {
        self.conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS memory_meta(
                key TEXT PRIMARY KEY,
                value TEXT NOT NULL
            );",
        )?;
        let dimensions = DIMENSIONS.to_string();
        let valid = self.meta("schema_version")?.as_deref() == Some(SCHEMA_VERSION)
            && self.meta("model_id")?.as_deref() == Some(MODEL_ID)
            && self.meta("dimensions")?.as_deref() == Some(dimensions.as_str());
        if valid {
            return Ok(());
        }

        self.conn.execute_batch(
            "DROP TABLE IF EXISTS vec_memory_chunks;
             DROP TABLE IF EXISTS memory_chunks;
             DROP TABLE IF EXISTS memory_files;
             DROP TABLE IF EXISTS memory_meta;
             CREATE TABLE memory_meta(
                key TEXT PRIMARY KEY,
                value TEXT NOT NULL
             );
             CREATE TABLE memory_files(
                id INTEGER PRIMARY KEY,
                rel_path TEXT UNIQUE NOT NULL,
                title TEXT NOT NULL,
                frontmatter_raw TEXT,
                modified_ns INTEGER NOT NULL,
                size_bytes INTEGER NOT NULL,
                content_fingerprint INTEGER NOT NULL
             );
             CREATE TABLE memory_chunks(
                id INTEGER PRIMARY KEY,
                file_id INTEGER NOT NULL REFERENCES memory_files(id) ON DELETE CASCADE,
                ordinal INTEGER NOT NULL,
                start_byte INTEGER NOT NULL,
                end_byte INTEGER NOT NULL,
                UNIQUE(file_id, ordinal)
             );
             CREATE VIRTUAL TABLE vec_memory_chunks USING vec0(
                chunk_id INTEGER PRIMARY KEY,
                embedding FLOAT[256] distance_metric=cosine
             );",
        )?;
        let transaction = self.conn.transaction()?;
        for (key, value) in [
            ("schema_version", SCHEMA_VERSION),
            ("model_id", MODEL_ID),
            ("dimensions", dimensions.as_str()),
        ] {
            transaction.execute(
                "INSERT INTO memory_meta(key, value) VALUES (?1, ?2)",
                params![key, value],
            )?;
        }
        transaction.commit()?;
        Ok(())
    }
}

fn register_sqlite_vec() -> Result<(), IndexError> {
    type ExtensionInitializer = unsafe extern "C" fn(
        *mut rusqlite::ffi::sqlite3,
        *mut *mut std::os::raw::c_char,
        *const rusqlite::ffi::sqlite3_api_routines,
    ) -> std::os::raw::c_int;
    let code = *SQLITE_VEC_REGISTRATION.get_or_init(|| {
        // SAFETY: Category 8 (FFI) and 13 (unsafe contract). sqlite-vec exports the
        // SQLite extension initializer ABI required by sqlite3_auto_extension; the
        // binding erases that signature, so this is the upstream-documented adapter.
        unsafe {
            sqlite3_auto_extension(Some(
                std::mem::transmute::<*const (), ExtensionInitializer>(
                    sqlite_vec::sqlite3_vec_init as *const (),
                ),
            ))
        }
    });
    if code == rusqlite::ffi::SQLITE_OK {
        Ok(())
    } else {
        Err(IndexError::Registration(code))
    }
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::{MemoryIndex, MODEL_ID};

    #[test]
    fn cache_rebuilds_when_model_metadata_changes() {
        // Given
        let root =
            std::env::temp_dir().join(format!("tcui-memory-index-{}", rand::random::<u64>()));
        fs::create_dir_all(&root).expect("temporary index root");
        let database = root.join("memory.sqlite3");
        let index = MemoryIndex::open(&database).expect("memory index");
        index
            .set_meta_for_test("model_id", "obsolete")
            .expect("obsolete model metadata");

        // When
        drop(index);
        let rebuilt = MemoryIndex::open(&database).expect("rebuilt memory index");

        // Then
        assert_eq!(
            rebuilt.meta("model_id").expect("model metadata").as_deref(),
            Some(MODEL_ID)
        );
        fs::remove_dir_all(root).expect("temporary index cleanup");
    }

    #[test]
    #[ignore = "manual benchmark"]
    fn benchmark_knn_with_ten_thousand_chunks() {
        // Given
        let root = std::env::temp_dir().join(format!("tcui-memory-knn-{}", rand::random::<u64>()));
        fs::create_dir_all(&root).expect("temporary index root");
        let database = root.join("memory.sqlite3");
        let mut index = MemoryIndex::open(&database).expect("memory index");
        let embedding = crate::memory::embedding::as_blob(&vec![0.01_f32; super::DIMENSIONS]);
        let transaction = index.conn.transaction().expect("index transaction");
        transaction
            .execute(
                "INSERT INTO memory_files(
                    rel_path, title, modified_ns, size_bytes, content_fingerprint
                 ) VALUES ('benchmark.md', 'Benchmark', 0, 0, 0)",
                [],
            )
            .expect("benchmark file");
        for ordinal in 0..10_000_i64 {
            transaction
                .execute(
                    "INSERT INTO memory_chunks(file_id, ordinal, start_byte, end_byte)
                     VALUES (1, ?1, 0, 1)",
                    [ordinal],
                )
                .expect("benchmark chunk");
            transaction
                .execute(
                    "INSERT INTO vec_memory_chunks(chunk_id, embedding) VALUES (?1, ?2)",
                    rusqlite::params![ordinal + 1, embedding],
                )
                .expect("benchmark vector");
        }
        transaction.commit().expect("benchmark commit");
        let mut statement = index
            .conn
            .prepare(
                "SELECT chunk_id FROM vec_memory_chunks
                 WHERE embedding MATCH ?1 AND k = 8",
            )
            .expect("KNN statement");
        let mut query = || {
            statement
                .query_map([&embedding], |row| row.get::<_, i64>(0))
                .and_then(|rows| rows.collect::<rusqlite::Result<Vec<_>>>())
                .expect("KNN query")
        };
        assert_eq!(query().len(), 8);

        // When
        let started = std::time::Instant::now();
        for _ in 0..100 {
            assert_eq!(query().len(), 8);
        }
        let average = started.elapsed() / 100;

        // Then
        eprintln!("10,000-chunk KNN average: {average:?}");
        fs::remove_dir_all(root).expect("temporary index cleanup");
    }
}
