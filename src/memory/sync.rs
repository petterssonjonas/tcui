use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

use rusqlite::{params, OptionalExtension};

use super::embedding::{as_blob, embed_many};
use super::index::MemoryIndex;
use super::markdown::{parse_memory, MemoryChunk};
use super::paths::MemoryPaths;
use super::store::MemoryError;

static SYNC_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

struct SourceFile {
    relative: PathBuf,
    title: String,
    frontmatter_raw: Option<String>,
    modified_ns: i64,
    size_bytes: i64,
    fingerprint: i64,
    content: String,
    chunks: Vec<MemoryChunk>,
}

struct ExistingFile {
    modified_ns: i64,
    size_bytes: i64,
    fingerprint: i64,
}

struct PreparedFile {
    source: SourceFile,
    embeddings: Vec<Vec<f32>>,
}

pub(super) fn synchronize(paths: &MemoryPaths, database: &Path) -> Result<(), MemoryError> {
    let _guard = SYNC_LOCK
        .lock()
        .map_err(|_| MemoryError::Invalid("memory synchronization lock failed".to_string()))?;
    let sources = scan_sources(paths)?;
    let mut index = MemoryIndex::open(database)?;
    let existing = existing_files(&index)?;
    let source_names = sources
        .iter()
        .map(|source| relative_text(&source.relative))
        .collect::<HashSet<_>>();
    let mut prepared = Vec::new();
    for source in sources {
        let name = relative_text(&source.relative);
        let unchanged = existing.get(&name).is_some_and(|row| {
            row.modified_ns == source.modified_ns
                && row.size_bytes == source.size_bytes
                && row.fingerprint == source.fingerprint
        });
        if !unchanged {
            let texts = source
                .chunks
                .iter()
                .filter_map(|chunk| source.content.get(chunk.start_byte..chunk.end_byte))
                .map(str::to_string)
                .collect::<Vec<_>>();
            prepared.push(PreparedFile {
                embeddings: embed_many(&texts)?,
                source,
            });
        }
    }

    let transaction = index.conn.transaction()?;
    for name in existing.keys().filter(|name| !source_names.contains(*name)) {
        delete_file(&transaction, name)?;
    }
    for file in prepared {
        let name = relative_text(&file.source.relative);
        delete_vectors(&transaction, &name)?;
        let file_id = transaction.query_row(
            "INSERT INTO memory_files(
                rel_path, title, frontmatter_raw, modified_ns, size_bytes, content_fingerprint
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6)
             ON CONFLICT(rel_path) DO UPDATE SET
                title = excluded.title,
                frontmatter_raw = excluded.frontmatter_raw,
                modified_ns = excluded.modified_ns,
                size_bytes = excluded.size_bytes,
                content_fingerprint = excluded.content_fingerprint
             RETURNING id",
            params![
                name,
                file.source.title,
                file.source.frontmatter_raw,
                file.source.modified_ns,
                file.source.size_bytes,
                file.source.fingerprint,
            ],
            |row| row.get::<_, i64>(0),
        )?;
        transaction.execute("DELETE FROM memory_chunks WHERE file_id = ?1", [file_id])?;
        for (ordinal, (chunk, embedding)) in
            file.source.chunks.iter().zip(file.embeddings).enumerate()
        {
            transaction.execute(
                "INSERT INTO memory_chunks(file_id, ordinal, start_byte, end_byte)
                 VALUES (?1, ?2, ?3, ?4)",
                params![
                    file_id,
                    i64::try_from(ordinal).unwrap_or(i64::MAX),
                    i64::try_from(chunk.start_byte).unwrap_or(i64::MAX),
                    i64::try_from(chunk.end_byte).unwrap_or(i64::MAX),
                ],
            )?;
            let chunk_id = transaction.last_insert_rowid();
            transaction.execute(
                "INSERT INTO vec_memory_chunks(chunk_id, embedding) VALUES (?1, ?2)",
                params![chunk_id, as_blob(&embedding)],
            )?;
        }
    }
    transaction.commit()?;
    Ok(())
}

fn scan_sources(paths: &MemoryPaths) -> Result<Vec<SourceFile>, MemoryError> {
    let mut sources = Vec::new();
    for entry in walkdir::WalkDir::new(paths.root()).follow_links(false) {
        let entry = entry.map_err(|error| MemoryError::Walk(error.to_string()))?;
        if entry.file_type().is_symlink()
            || !entry.file_type().is_file()
            || entry.path().extension().and_then(|value| value.to_str()) != Some("md")
        {
            continue;
        }
        let relative = entry
            .path()
            .strip_prefix(paths.root())
            .map_err(|_| MemoryError::Invalid("memory path escaped its root".to_string()))?
            .to_path_buf();
        let confined = paths.existing_target(&relative)?;
        let content = std::fs::read_to_string(&confined.absolute)?;
        let metadata = std::fs::metadata(&confined.absolute)?;
        let fallback = confined
            .relative
            .file_stem()
            .and_then(|value| value.to_str())
            .unwrap_or("Memory");
        let parsed = parse_memory(&content, fallback);
        sources.push(SourceFile {
            relative: confined.relative,
            title: parsed.title,
            frontmatter_raw: parsed.frontmatter_raw,
            modified_ns: modified_ns(&metadata),
            size_bytes: i64::try_from(metadata.len()).unwrap_or(i64::MAX),
            fingerprint: fingerprint(content.as_bytes()),
            content,
            chunks: parsed.chunks,
        });
    }
    sources.sort_by(|left, right| left.relative.cmp(&right.relative));
    Ok(sources)
}

fn existing_files(index: &MemoryIndex) -> Result<HashMap<String, ExistingFile>, MemoryError> {
    let mut statement = index.conn.prepare(
        "SELECT rel_path, modified_ns, size_bytes, content_fingerprint FROM memory_files",
    )?;
    let rows = statement.query_map([], |row| {
        Ok((
            row.get::<_, String>(0)?,
            ExistingFile {
                modified_ns: row.get(1)?,
                size_bytes: row.get(2)?,
                fingerprint: row.get(3)?,
            },
        ))
    })?;
    Ok(rows.collect::<rusqlite::Result<HashMap<_, _>>>()?)
}

fn delete_vectors(transaction: &rusqlite::Transaction<'_>, name: &str) -> rusqlite::Result<()> {
    let file_id = transaction
        .query_row(
            "SELECT id FROM memory_files WHERE rel_path = ?1",
            [name],
            |row| row.get::<_, i64>(0),
        )
        .optional()?;
    if let Some(file_id) = file_id {
        transaction.execute(
            "DELETE FROM vec_memory_chunks
             WHERE chunk_id IN (SELECT id FROM memory_chunks WHERE file_id = ?1)",
            [file_id],
        )?;
    }
    Ok(())
}

fn delete_file(transaction: &rusqlite::Transaction<'_>, name: &str) -> rusqlite::Result<()> {
    delete_vectors(transaction, name)?;
    transaction.execute("DELETE FROM memory_files WHERE rel_path = ?1", [name])?;
    Ok(())
}

fn relative_text(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

fn modified_ns(metadata: &std::fs::Metadata) -> i64 {
    metadata
        .modified()
        .ok()
        .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
        .and_then(|duration| i64::try_from(duration.as_nanos()).ok())
        .unwrap_or(0)
}

pub(super) fn fingerprint(bytes: &[u8]) -> i64 {
    let hash = bytes.iter().fold(0xcbf29ce484222325_u64, |hash, byte| {
        (hash ^ u64::from(*byte)).wrapping_mul(0x100000001b3)
    });
    i64::from_ne_bytes(hash.to_ne_bytes())
}
