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
    std::fs::create_dir_all(destination)?;
    let extension = match format {
        OutputFormat::Markdown => "md",
        OutputFormat::Json => "json",
    };
    let path = unique_export_path(destination, &document.title, document.id, extension)?;
    std::fs::write(&path, render_chat_document(document, format)?)?;
    Ok(path)
}

#[cfg(feature = "memory")]
pub fn export_memory_document_to_dir(
    document: &crate::memory::MemoryDocument,
    format: OutputFormat,
    destination: &Path,
) -> Result<PathBuf> {
    std::fs::create_dir_all(destination)?;
    let extension = match format {
        OutputFormat::Markdown => "md",
        OutputFormat::Json => "json",
    };
    let id = i64::try_from(document.id).unwrap_or(i64::MAX);
    let path = unique_export_path(destination, &document.title, id, extension)?;
    std::fs::write(&path, render_memory_document(document, format)?)?;
    Ok(path)
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

pub fn unique_export_path(dir: &Path, title: &str, id: i64, extension: &str) -> Result<PathBuf> {
    let base = sanitize_filename(title);
    let mut candidate = dir.join(format!("{base}-{id}.{extension}"));
    let mut suffix = 2_u32;
    while candidate.exists() {
        candidate = dir.join(format!("{base}-{id}-{suffix}.{extension}"));
        suffix += 1;
    }
    Ok(candidate)
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
