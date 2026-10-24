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
        crate::protection::require(crate::protection::Domain::Memories)?;
        let _guard = self.lock_mutations()?;
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
            id: self.allocate_document_id()?,
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
        self.require_native_policy()?;
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
        crate::protection::require(crate::protection::Domain::Memories)?;
        let _guard = self.lock_mutations()?;
        let logical = self.paths.logical_path(path)?;
        let document = self
            .find_document_by_logical_path(&logical)?
            .ok_or_else(|| MemoryError::Invalid("memory path is unavailable".to_string()))?;
        let source = self.paths.active_document_path(document.id);
        self.trash_document(&source, &document)
    }

    pub(super) fn trash_document(
        &self,
        source: &Path,
        document: &MemoryDocument,
    ) -> Result<PathBuf, MemoryError> {
        if source.parent() != Some(self.paths.root()) {
            return Err(MemoryError::Invalid(
                "memory source is outside the native root".to_string(),
            ));
        }
        let name = source
            .file_name()
            .ok_or_else(|| MemoryError::Invalid("memory source has no filename".to_string()))?;
        let (root, trash) = self.paths.directories()?;
        if !root.symlink_metadata(name)?.is_file() {
            return Err(MemoryError::Invalid(
                "memory source is not a regular file".to_string(),
            ));
        }
        let mut suffix = 0u64;
        let destination_name = loop {
            let candidate = if suffix == 0 {
                format!("{:016x}.tcui-memory", document.id)
            } else {
                format!("{:016x}-{suffix}.tcui-memory", document.id)
            };
            match publish_trash(&root, name, &trash, std::ffi::OsStr::new(&candidate)) {
                Ok(()) => break candidate,
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                    suffix = suffix.checked_add(1).ok_or_else(|| {
                        MemoryError::Invalid("memory Trash name space is exhausted".to_string())
                    })?;
                }
                Err(error) => return Err(error.into()),
            }
        };
        let destination = self.paths.root().join(".trash").join(destination_name);
        let repair = || -> Result<(), MemoryError> {
            rustix::fs::fsync(&trash).map_err(std::io::Error::from)?;
            rustix::fs::fsync(&root).map_err(std::io::Error::from)?;
            self.sync()
        };
        repair().map_err(|error| MemoryError::RemovedButNotIndexed {
            path: document.logical_path.to_string_lossy().into_owned(),
            reason: error.to_string(),
        })?;
        Ok(destination)
    }

    #[allow(dead_code)]
    pub(crate) fn add_from_plaintext(&self, path: &Path) -> Result<WriteOutcome, MemoryError> {
        crate::protection::require(crate::protection::Domain::Memories)?;
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
        crate::protection::require(crate::protection::Domain::Memories)?;
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

pub(super) fn validate_markdown(markdown: &str) -> Result<(), MemoryError> {
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

fn publish_trash(
    root: &cap_std::fs::Dir,
    name: &std::ffi::OsStr,
    trash: &cap_std::fs::Dir,
    destination: &std::ffi::OsStr,
) -> std::io::Result<()> {
    #[cfg(any(target_os = "linux", target_os = "android"))]
    rustix::fs::renameat_with(
        root,
        name,
        trash,
        destination,
        rustix::fs::RenameFlags::NOREPLACE,
    )?;
    #[cfg(not(any(target_os = "linux", target_os = "android")))]
    {
        // A hard link publishes without replacing any existing Trash entry.
        // Persist that recovery copy before removing the active directory entry.
        rustix::fs::linkat(root, name, trash, destination, rustix::fs::AtFlags::empty())?;
        rustix::fs::fsync(trash)?;
        rustix::fs::unlinkat(root, name, rustix::fs::AtFlags::empty())?;
    }
    Ok(())
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
    fn concurrent_trash_publication_never_replaces_the_winner_or_removes_the_loser() {
        use std::ffi::OsStr;

        let path =
            std::env::temp_dir().join(format!("tcui-memory-trash-race-{}", rand::random::<u64>()));
        let root = crate::protection::open_native_directory(&path, true).expect("native root");
        let trash =
            crate::protection::open_native_directory(&path.join(".trash"), true).expect("Trash");
        std::fs::write(path.join("first"), b"first source").expect("first source");
        std::fs::write(path.join("second"), b"second source").expect("second source");
        let gate = std::sync::Barrier::new(2);
        let (first, second) = std::thread::scope(|scope| {
            let first = scope.spawn(|| {
                gate.wait();
                super::publish_trash(&root, OsStr::new("first"), &trash, OsStr::new("shared"))
            });
            let second = scope.spawn(|| {
                gate.wait();
                super::publish_trash(&root, OsStr::new("second"), &trash, OsStr::new("shared"))
            });
            (
                first.join().expect("first publisher"),
                second.join().expect("second publisher"),
            )
        });
        let (winner, loser, failure) = match (first, second) {
            (Ok(()), Err(error)) => ("first", "second", error),
            (Err(error), Ok(())) => ("second", "first", error),
            results => panic!("expected one winner: {results:?}"),
        };
        assert_eq!(failure.kind(), std::io::ErrorKind::AlreadyExists);
        assert_eq!(
            std::fs::read(path.join(".trash/shared")).expect("winning recovery body"),
            format!("{winner} source").as_bytes()
        );
        assert_eq!(
            std::fs::read(path.join(loser)).expect("losing source retained"),
            format!("{loser} source").as_bytes()
        );
        assert!(!path.join(winner).exists());
        drop(trash);
        drop(root);
        std::fs::remove_dir_all(path).expect("fixture cleanup");
    }

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
