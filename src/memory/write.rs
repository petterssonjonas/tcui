use std::path::{Path, PathBuf};

use super::markdown::parse_memory;
use super::store::{
    is_memory_document_path, normalize_markdown, now_ms, MemoryDocument, MemoryError, MemoryStore,
};
use super::sync::fingerprint;

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub(crate) enum WriteOutcome {
    Saved { title: String, path: String },
    AlreadyKnown { title: String },
}

impl MemoryStore {
    pub(crate) fn write(
        &self,
        path: &Path,
        markdown: &str,
        overwrite: bool,
    ) -> Result<WriteOutcome, MemoryError> {
        if markdown.trim().is_empty() {
            return Err(MemoryError::Invalid("memory Markdown is empty".to_string()));
        }
        let logical_path = self.paths.logical_path(path)?;
        let normalized_markdown = normalize_markdown(markdown);
        if let Some((physical_path, mut document)) = self
            .find_document_by_logical_path(&logical_path)?
            .map(|document| (self.paths.active_document_path(document.id), document))
        {
            if !overwrite {
                return Err(MemoryError::Invalid("memory already exists".to_string()));
            }
            document.title = parse_memory(
                &normalized_markdown,
                logical_path
                    .file_stem()
                    .and_then(|value| value.to_str())
                    .unwrap_or("Memory"),
            )
            .title;
            document.updated_at_ms = now_ms();
            document.markdown = normalized_markdown;
            self.write_document_at(&physical_path, &document)?;
            self.sync()?;
            return Ok(WriteOutcome::Saved {
                title: document.title,
                path: logical_path.to_string_lossy().replace('\\', "/"),
            });
        }
        let fallback = logical_path
            .file_stem()
            .and_then(|value| value.to_str())
            .unwrap_or("Memory");
        let title = parse_memory(&normalized_markdown, fallback).title;
        let now = now_ms();
        let document = MemoryDocument {
            schema_version: 1,
            id: self.allocate_document_id(),
            logical_path: logical_path.clone(),
            title: title.clone(),
            created_at_ms: now,
            updated_at_ms: now,
            markdown: normalized_markdown,
        };
        self.write_document_at(&self.paths.active_document_path(document.id), &document)?;
        self.sync()?;
        Ok(WriteOutcome::Saved {
            title,
            path: logical_path.to_string_lossy().replace('\\', "/"),
        })
    }

    pub(crate) fn remember(&self, fact: &str) -> Result<WriteOutcome, MemoryError> {
        let fact = fact.trim();
        if fact.is_empty() {
            return Err(MemoryError::Invalid("memory is empty".to_string()));
        }
        if likely_secret(fact) {
            return Err(MemoryError::Invalid(
                "memory looks like a credential or private key".to_string(),
            ));
        }
        self.sync()?;
        if let Some(title) = self.find_duplicate(fact)? {
            return Ok(WriteOutcome::AlreadyKnown { title });
        }
        let title = memory_title(fact);
        let slug = slug(&title);
        let hash = u64::from_ne_bytes(fingerprint(fact.as_bytes()).to_ne_bytes());
        let path = PathBuf::from(format!("{slug}-{hash:016x}.md"));
        let escaped_title = title.replace('"', "\\\"");
        let markdown = format!(
            "---\ntitle: \"{escaped_title}\"\nsource: tcui\ncreated: {}\n---\n\n{fact}\n",
            chrono::Utc::now().to_rfc3339()
        );
        self.write(&path, &markdown, false)
    }

    pub(crate) fn forget(&self, path: &Path) -> Result<PathBuf, MemoryError> {
        self.sync()?;
        let logical = self.paths.logical_path(path)?;
        let document = self
            .find_document_by_logical_path(&logical)?
            .ok_or_else(|| MemoryError::Invalid("memory path is unavailable".to_string()))?;
        let source = self.paths.active_document_path(document.id);
        let destination = available_trash_path(self.paths.trash_document_path(document.id));
        std::fs::rename(source, &destination)?;
        self.sync()?;
        Ok(destination)
    }

    #[allow(dead_code)]
    pub(crate) fn add_from_plaintext(&self, path: &Path) -> Result<WriteOutcome, MemoryError> {
        let source = std::fs::canonicalize(path)?;
        let extension = source.extension().and_then(|value| value.to_str());
        if !matches!(extension, Some("md" | "txt")) {
            return Err(MemoryError::Invalid(
                "memory import accepts only .md or .txt files".to_string(),
            ));
        }
        let contents = std::fs::read_to_string(&source)?;
        let logical = PathBuf::from(format!(
            "{}.md",
            source
                .file_stem()
                .and_then(|value| value.to_str())
                .unwrap_or("memory")
        ));
        self.write(&logical, &contents, false)
    }

    fn find_duplicate(&self, fact: &str) -> Result<Option<String>, MemoryError> {
        let normalized_fact = normalize_markdown(fact).trim().to_string();
        for entry in std::fs::read_dir(self.paths.root())? {
            let entry = entry?;
            if !is_memory_document_path(&entry.path()) {
                continue;
            }
            let document = self.read_document_at(&entry.path())?;
            if memory_body(&document.markdown) == normalized_fact {
                return Ok(Some(document.title));
            }
        }
        Ok(None)
    }
}

fn available_trash_path(path: PathBuf) -> PathBuf {
    if !path.exists() {
        return path;
    }
    let stem = path
        .file_stem()
        .and_then(|value| value.to_str())
        .unwrap_or("memory");
    let extension = path
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or("md");
    path.with_file_name(format!("{stem}-{:016x}.{extension}", rand::random::<u64>()))
}

fn memory_body(markdown: &str) -> &str {
    let trimmed = markdown.trim();
    if let Some(rest) = trimmed.strip_prefix("---\n") {
        if let Some(end) = rest.find("\n---") {
            return rest[end + 4..].trim();
        }
    }
    trimmed
}

fn memory_title(fact: &str) -> String {
    fact.split_whitespace()
        .take(6)
        .collect::<Vec<_>>()
        .join(" ")
        .trim_matches(|character: char| !character.is_alphanumeric())
        .chars()
        .take(80)
        .collect()
}

fn slug(title: &str) -> String {
    let mut slug = String::new();
    for character in title.chars().flat_map(char::to_lowercase) {
        if character.is_ascii_alphanumeric() {
            slug.push(character);
        } else if !slug.is_empty() && !slug.ends_with('-') {
            slug.push('-');
        }
    }
    slug.trim_matches('-').chars().take(48).collect::<String>()
}

pub(crate) fn likely_secret(text: &str) -> bool {
    let lower = text.to_ascii_lowercase();
    [
        "-----begin private key",
        "-----begin rsa private key",
        "api_key=",
        "apikey=",
        "password=",
        "bearer ",
        "ghp_",
        "github_pat_",
        "sk-proj-",
        "sk_live_",
    ]
    .iter()
    .any(|marker| lower.contains(marker))
}

#[cfg(test)]
mod tests {
    use super::likely_secret;

    #[test]
    fn credential_patterns_are_rejected_without_rejecting_preferences() {
        // Given / When / Then
        assert!(likely_secret("api_key=secret"));
        assert!(likely_secret("-----BEGIN PRIVATE KEY-----"));
        assert!(likely_secret("Bearer token-value"));
        assert!(likely_secret("github_pat_example"));
        assert!(!likely_secret("User prefers concise technical answers."));
    }
}
