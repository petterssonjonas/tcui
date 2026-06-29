use std::fs::OpenOptions;
use std::io::Write;
use std::path::{Path, PathBuf};

use super::markdown::parse_memory;
use super::store::{MemoryError, MemoryStore};
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
        let target = self.paths.write_target(path)?;
        if target.absolute.exists() && !overwrite {
            return Err(MemoryError::Invalid("memory already exists".to_string()));
        }
        if let Some(parent) = target.absolute.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let target = self.paths.write_target(&target.relative)?;
        let temporary = temporary_path(&target.absolute);
        let write_result = (|| -> Result<(), MemoryError> {
            let mut file = OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&temporary)?;
            file.write_all(markdown.as_bytes())?;
            file.sync_all()?;
            std::fs::rename(&temporary, &target.absolute)?;
            Ok(())
        })();
        if write_result.is_err() {
            let _ = std::fs::remove_file(&temporary);
        }
        write_result?;
        self.sync()?;
        let fallback = target
            .relative
            .file_stem()
            .and_then(|value| value.to_str())
            .unwrap_or("Memory");
        Ok(WriteOutcome::Saved {
            title: parse_memory(markdown, fallback).title,
            path: target.relative.to_string_lossy().replace('\\', "/"),
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
        let target = self.paths.existing_target(path)?;
        let vault = self.paths.root().parent().unwrap_or(self.paths.root());
        let base = vault.join(".trash/tcui-memory").join(&target.relative);
        if let Some(parent) = base.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let destination = available_trash_path(base);
        std::fs::rename(target.absolute, &destination)?;
        self.sync()?;
        Ok(destination)
    }

    fn find_duplicate(&self, fact: &str) -> Result<Option<String>, MemoryError> {
        for entry in walkdir::WalkDir::new(self.paths.root()).follow_links(false) {
            let entry = entry.map_err(|error| MemoryError::Walk(error.to_string()))?;
            if !entry.file_type().is_file()
                || entry.path().extension().and_then(|value| value.to_str()) != Some("md")
            {
                continue;
            }
            let content = std::fs::read_to_string(entry.path())?;
            if memory_body(&content) == fact {
                let fallback = entry
                    .path()
                    .file_stem()
                    .and_then(|value| value.to_str())
                    .unwrap_or("Memory");
                return Ok(Some(parse_memory(&content, fallback).title));
            }
        }
        Ok(None)
    }
}

fn temporary_path(target: &Path) -> PathBuf {
    let name = target
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or("memory.md");
    target.with_file_name(format!(".{name}.tmp-{:016x}", rand::random::<u64>()))
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
