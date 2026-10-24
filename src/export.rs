use std::path::{Path, PathBuf};

use color_eyre::Result;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutputFormat {
    Markdown,
    Json,
}

pub fn export_chat_document_to_dir(
    document: &crate::storage::chat_store::ChatDocument,
    format: OutputFormat,
    destination: &Path,
) -> Result<PathBuf> {
    let extension = match format {
        OutputFormat::Markdown => "md",
        OutputFormat::Json => "json",
    };
    create_named_export(
        destination,
        &document.title,
        document.id,
        extension,
        render_chat_document(document, format)?.as_bytes(),
    )
}

#[cfg(feature = "memory")]
pub fn export_memory_document_to_dir(
    document: &crate::memory::MemoryDocument,
    format: OutputFormat,
    destination: &Path,
) -> Result<PathBuf> {
    let extension = match format {
        OutputFormat::Markdown => "md",
        OutputFormat::Json => "json",
    };
    let id = i64::try_from(document.id).unwrap_or(i64::MAX);
    create_named_export(
        destination,
        &document.title,
        id,
        extension,
        render_memory_document(document, format)?.as_bytes(),
    )
}

pub fn render_chat_document(
    document: &crate::storage::chat_store::ChatDocument,
    format: OutputFormat,
) -> Result<String> {
    match format {
        OutputFormat::Json => Ok(serde_json::to_string_pretty(document)?),
        OutputFormat::Markdown => {
            let mut output = format!(
                "# {}\n\nCreated: {}\nUpdated: {}\n\n",
                document.title,
                format_timestamp(document.created_at_ms),
                format_timestamp(document.updated_at_ms),
            );
            for message in &document.messages {
                output.push_str(&format!("## {}\n\n{}\n\n", message.role, message.content));
                if let Some(thinking) = &message.thinking_content {
                    output.push_str(&format!(
                        "<details>\n<summary>Thinking</summary>\n\n{}\n\n</details>\n\n",
                        thinking
                    ));
                }
            }
            Ok(output)
        }
    }
}

#[cfg(feature = "memory")]
pub fn render_memory_document(
    document: &crate::memory::MemoryDocument,
    format: OutputFormat,
) -> Result<String> {
    match format {
        OutputFormat::Json => Ok(serde_json::to_string_pretty(document)?),
        OutputFormat::Markdown => Ok(document.markdown.clone()),
    }
}

pub(crate) fn create_named_export(
    dir: &Path,
    title: &str,
    id: i64,
    extension: &str,
    bytes: &[u8],
) -> Result<PathBuf> {
    let base = sanitize_filename(title);
    for suffix in 1..=u32::MAX {
        let filename = if suffix == 1 {
            format!("{base}-{id}.{extension}")
        } else {
            format!("{base}-{id}-{suffix}.{extension}")
        };
        let path = dir.join(filename);
        match crate::vault::create_artifact_path(&path, bytes) {
            Ok(()) => return Ok(path),
            Err(error)
                if error
                    .downcast_ref::<std::io::Error>()
                    .is_some_and(|source| source.kind() == std::io::ErrorKind::AlreadyExists) => {}
            Err(error) => return Err(error),
        }
    }
    Err(std::io::Error::new(
        std::io::ErrorKind::AlreadyExists,
        "No export filename is available",
    )
    .into())
}

pub fn sanitize_filename(title: &str) -> String {
    let sanitized = title
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() {
                character.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect::<String>();
    let collapsed = sanitized
        .split('-')
        .filter(|segment| !segment.is_empty())
        .collect::<Vec<_>>()
        .join("-");
    if collapsed.is_empty() {
        "memory".to_string()
    } else {
        collapsed
    }
}

pub fn format_timestamp(timestamp_ms: i64) -> String {
    chrono::DateTime::<chrono::Utc>::from_timestamp_millis(timestamp_ms)
        .map(|value| value.to_rfc3339())
        .unwrap_or_else(|| "unknown".to_string())
}
