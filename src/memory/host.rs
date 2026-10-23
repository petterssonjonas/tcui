use std::path::Path;

use super::recall::format_recall_context;
use super::store::{MemoryError, MemoryStore};
use super::MemoryActivity;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct MemoryRecall {
    pub(crate) context: String,
    pub(crate) titles: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SkillOperation {
    pub(crate) context: String,
    pub(crate) activity: Option<MemoryActivity>,
}

pub(crate) fn recall(
    config: &crate::config::AppConfig,
    query: &str,
) -> Result<MemoryRecall, MemoryError> {
    let store = configured_store(config)?;
    let hits = store.search(query, 8)?;
    let (context, titles) = format_recall_context(
        &hits,
        usize::from(config.memory.max_memories),
        config.memory.max_context_chars,
        config.memory.min_similarity,
    );
    Ok(MemoryRecall { context, titles })
}

// Disambiguate a memory filename from a request to copy an ephemeral chat.
// This parser grants no authority to execute an operation.
pub(crate) fn is_memory_file_operation(request: &str) -> bool {
    let mut words = request.split_whitespace();
    matches!(
        words.next(),
        Some("@memory" | "@memorize" | "@remember" | "@recall")
    ) && matches!(words.next(), Some("read" | "write" | "forget"))
        && words.next().is_some_and(|path| path.ends_with(".md"))
}

pub(crate) fn append_recall(messages: &mut [crate::app::Message], context: &str) {
    if context.is_empty() {
        return;
    }
    if let Some(message) = messages
        .iter_mut()
        .rev()
        .find(|message| message.role == "user")
    {
        message.content.push_str(context);
    }
}

pub(super) fn configured_store(
    config: &crate::config::AppConfig,
) -> Result<MemoryStore, MemoryError> {
    if !config.memory.enabled {
        return Err(MemoryError::Invalid("memory is disabled".to_string()));
    }
    let raw_vault = config
        .vault_path
        .as_deref()
        .map(Path::new)
        .ok_or_else(|| MemoryError::Invalid("Markdown Vault is not configured".to_string()))?;
    let vault =
        crate::app::generated_file::expand_user_path(raw_vault, dirs::home_dir().as_deref());
    MemoryStore::open(&vault, &MemoryStore::default_cache_path())
}
