use std::path::{Path, PathBuf};

use super::markdown::parse_memory;
use super::metadata::MemoryMetadata;
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
        self.write_with_metadata(path, markdown, overwrite, None)
    }

    fn write_with_metadata(
        &self,
        path: &Path,
        markdown: &str,
        overwrite: bool,
        metadata: Option<MemoryMetadata>,
    ) -> Result<WriteOutcome, MemoryError> {
        validate_markdown(markdown)?;
        if let Some(metadata) = &metadata {
            metadata.validate()?;
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
            if metadata.is_some() {
                document.metadata = metadata;
            }
            self.write_document_at(&physical_path, &document)?;
            return self.index_saved(document.title, &logical_path);
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
            metadata,
        };
        self.write_document_at(&self.paths.active_document_path(document.id), &document)?;
        self.index_saved(title, &logical_path)
    }

    fn index_saved(&self, title: String, path: &Path) -> Result<WriteOutcome, MemoryError> {
        let path = path.to_string_lossy().replace('\\', "/");
        self.sync()
            .map_err(|error| MemoryError::SavedButNotIndexed {
                title: title.clone(),
                path: path.clone(),
                reason: error.to_string(),
            })?;
        Ok(WriteOutcome::Saved { title, path })
    }

    /// Saves a contextual Markdown note with caller-supplied, validated provenance.
    pub(crate) fn remember_context(
        &self,
        markdown: &str,
        metadata: MemoryMetadata,
    ) -> Result<WriteOutcome, MemoryError> {
        validate_markdown(markdown)?;
        metadata.validate()?;
        if likely_secret(markdown) {
            return Err(MemoryError::Invalid(
                "memory looks like a credential or private key".to_string(),
            ));
        }
        if let Some(title) = self.find_duplicate(markdown, &metadata)? {
            return Ok(WriteOutcome::AlreadyKnown { title });
        }
        let title = parse_memory(markdown, &memory_title(markdown)).title;
        let identity = serde_json::to_vec(&(
            memory_body(&normalize_markdown(markdown)),
            metadata.scope,
            metadata.machine.as_ref().map(|machine| &machine.id),
        ))?;
        let hash = u64::from_ne_bytes(fingerprint(&identity).to_ne_bytes());
        let path = PathBuf::from(format!("{}-{hash:016x}.md", slug(&title)));
        self.write_with_metadata(&path, markdown, false, Some(metadata))
    }

    pub(crate) fn forget(&self, path: &Path) -> Result<PathBuf, MemoryError> {
        let logical = self.paths.logical_path(path)?;
        let document = self
            .find_document_by_logical_path(&logical)?
            .ok_or_else(|| MemoryError::Invalid("memory path is unavailable".to_string()))?;
        let source = self.paths.active_document_path(document.id);
        let destination = available_trash_path(self.paths.trash_document_path(document.id));
        std::fs::rename(source, &destination)?;
        self.sync()
            .map_err(|error| MemoryError::RemovedButNotIndexed {
                path: logical.to_string_lossy().into_owned(),
                reason: error.to_string(),
            })?;
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

    fn find_duplicate(
        &self,
        fact: &str,
        metadata: &MemoryMetadata,
    ) -> Result<Option<String>, MemoryError> {
        let normalized_fact = normalize_markdown(fact);
        let normalized_fact = memory_body(&normalized_fact);
        let legacy_metadata = MemoryMetadata::default();
        for entry in std::fs::read_dir(self.paths.root())? {
            let entry = entry?;
            if !is_memory_document_path(&entry.path()) {
                continue;
            }
            let document = self.read_document_at(&entry.path())?;
            let existing_metadata = document.metadata.as_ref().unwrap_or(&legacy_metadata);
            if metadata.same_scope(existing_metadata)
                && memory_body(&document.markdown) == normalized_fact
            {
                return Ok(Some(document.title));
            }
        }
        Ok(None)
    }
}

fn validate_markdown(markdown: &str) -> Result<(), MemoryError> {
    if markdown.len() > 1024 * 1024 {
        return Err(MemoryError::Invalid(
            "memory Markdown exceeds the 1 MiB input limit".to_string(),
        ));
    }
    if markdown.trim().is_empty() {
        return Err(MemoryError::Invalid("memory Markdown is empty".to_string()));
    }
    Ok(())
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
        "-----begin openssh private key",
        "-----begin ec private key",
        "-----begin dsa private key",
        "-----begin encrypted private key",
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
        || lower.lines().any(|line| {
            let Some((key, value)) = line.split_once(['=', ':']) else {
                return false;
            };
            let key = key.trim_matches(|character: char| {
                character.is_whitespace() || matches!(character, '-' | '*' | '`' | '"' | '\'')
            });
            matches!(
                key,
                "api_key" | "apikey" | "api key" | "password" | "access_token" | "client_secret"
            ) && !value.trim().is_empty()
        })
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

    #[test]
    fn markdown_credentials_and_private_key_variants_are_rejected() {
        // These are synthetic header markers, not a PEM block or key material.
        for secret in [
            concat!("-----BEGIN OPENSSH ", "PRIVATE KEY-----"),
            concat!("-----BEGIN EC ", "PRIVATE KEY-----"),
            concat!("-----BEGIN ENCRYPTED ", "PRIVATE KEY-----"),
            "- **password**: hunter2",
            "api_key : abc123",
            "\"client_secret\" = \"abc123\"",
            "access_token: abc123",
        ] {
            assert!(likely_secret(secret), "{secret}");
        }
    }
}
