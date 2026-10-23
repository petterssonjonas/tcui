use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};

use color_eyre::{eyre::eyre, Result};
use rusqlite::Connection;
use serde::{Deserialize, Serialize};

use crate::app::message::Message;
use crate::storage::crypto::{
    decrypt_shared_text_with_key, read_encrypted_document, write_encrypted_document, SharedKey,
};
use crate::storage::db::ConversationEntry;
use crate::storage::paths::TcuiDataPaths;

const CHAT_DOCUMENT_KIND: &str = "chat";
const CHAT_DOCUMENT_SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct ChatDocument {
    pub(crate) schema_version: u32,
    pub(crate) id: i64,
    pub(crate) tab_id: i64,
    pub(crate) title: String,
    pub(crate) created_at_ms: i64,
    pub(crate) updated_at_ms: i64,
    pub(crate) pinned: bool,
    pub(crate) messages: Vec<Message>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) parent: Option<ChatLineage>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct ChatLineage {
    pub(crate) conversation_id: i64,
    pub(crate) last_message_id: Option<i64>,
    pub(crate) copied_message_count: usize,
}

impl ChatLineage {
    pub(crate) fn from_messages(conversation_id: i64, messages: &[Message]) -> Self {
        Self {
            conversation_id,
            last_message_id: messages.iter().rev().find_map(|message| message.id),
            copied_message_count: messages.len(),
        }
    }
}

#[derive(Debug, Clone)]
pub(crate) struct ChatStore {
    paths: TcuiDataPaths,
    key: SharedKey,
    ephemeral: Arc<Mutex<HashMap<i64, ChatDocument>>>,
}

impl ChatStore {
    pub(crate) fn new(paths: TcuiDataPaths, key: SharedKey) -> Self {
        Self {
            paths,
            key,
            ephemeral: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    pub(crate) fn create_conversation(&self, tab_id: i64) -> Result<i64> {
        Ok(self.create_snapshot(tab_id, "New Chat", &[], None)?.id)
    }

    pub(crate) fn create_snapshot(
        &self,
        tab_id: i64,
        title: &str,
        messages: &[Message],
        parent: Option<ChatLineage>,
    ) -> Result<ChatDocument> {
        let document = snapshot_document(self.allocate_id(), tab_id, title, messages, parent);
        self.write_document(&document)?;
        Ok(document)
    }

    pub(crate) fn create_ephemeral_snapshot(
        &self,
        tab_id: i64,
        title: &str,
        messages: &[Message],
        parent: ChatLineage,
    ) -> Result<i64> {
        // Negative IDs never fall back to disk, even after deletion or reopening storage.
        static NEXT_EPHEMERAL_ID: AtomicI64 = AtomicI64::new(-1);
        let id = NEXT_EPHEMERAL_ID
            .try_update(Ordering::Relaxed, Ordering::Relaxed, |id| id.checked_sub(1))
            .map_err(|_| eyre!("ephemeral conversation IDs exhausted"))?;
        let document = snapshot_document(id, tab_id, title, messages, Some(parent));
        self.ephemeral_documents()?.insert(id, document);
        Ok(id)
    }

    pub(crate) fn save_message(&self, msg: &Message) -> Result<i64> {
        self.mutate_document(msg.conversation_id, |document| {
            let next_id = document
                .messages
                .iter()
                .filter_map(|message| message.id)
                .max()
                .unwrap_or(0)
                + 1;
            let mut stored = msg.clone();
            stored.id = Some(next_id);
            stored.conversation_id = document.id;
            document.messages.push(stored);
            next_id
        })
    }

    pub(crate) fn get_messages(&self, conversation_id: i64) -> Result<Vec<Message>> {
        Ok(self.read_document(conversation_id)?.messages)
    }

    pub(crate) fn replace_messages(
        &self,
        conversation_id: i64,
        messages: &[Message],
    ) -> Result<()> {
        self.mutate_document(conversation_id, |document| {
            let mut next_id = 1_i64;
            document.messages = messages
                .iter()
                .cloned()
                .map(|mut message| {
                    message.conversation_id = conversation_id;
                    message.id = Some(message.id.unwrap_or_else(|| {
                        let assigned = next_id;
                        next_id += 1;
                        assigned
                    }));
                    if let Some(existing_id) = message.id {
                        next_id = next_id.max(existing_id + 1);
                    }
                    message
                })
                .collect();
        })
    }

    pub(crate) fn get_conversations(&self, tab_id: i64) -> Result<Vec<ConversationEntry>> {
        let (conversations, _) = self.get_conversations_with_warnings(tab_id)?;
        Ok(conversations)
    }

    pub(crate) fn get_conversations_with_warnings(
        &self,
        tab_id: i64,
    ) -> Result<(Vec<ConversationEntry>, usize)> {
        let mut conversations = Vec::new();
        let mut skipped = 0_usize;
        if !self.paths.chats_dir.exists() {
            return Ok((conversations, skipped));
        }

        for entry in std::fs::read_dir(&self.paths.chats_dir)? {
            let entry = entry?;
            let path = entry.path();
            if !is_chat_document_path(&path) {
                continue;
            }

            match self.read_document_from_path(&path) {
                Ok(document) if document.tab_id == tab_id => {
                    conversations.push(ConversationEntry {
                        id: document.id,
                        title: document.title,
                        created_at: String::new(),
                        updated_at_ms: document.updated_at_ms,
                        pinned: document.pinned,
                    })
                }
                Ok(_) => {}
                Err(_) => skipped += 1,
            }
        }

        conversations.sort_by(|left, right| {
            right
                .pinned
                .cmp(&left.pinned)
                .then_with(|| right.updated_at_ms.cmp(&left.updated_at_ms))
                .then_with(|| left.id.cmp(&right.id))
        });
        Ok((conversations, skipped))
    }

    pub(crate) fn list_all_documents(&self) -> Result<Vec<ChatDocument>> {
        let mut documents = Vec::new();
        if !self.paths.chats_dir.exists() {
            return Ok(documents);
        }
        for entry in std::fs::read_dir(&self.paths.chats_dir)? {
            let entry = entry?;
            let path = entry.path();
            if !is_chat_document_path(&path) {
                continue;
            }
            match self.read_document_from_path(&path) {
                Ok(document) => documents.push(document),
                Err(_) => eprintln!("warning: unreadable stored chat skipped"),
            }
        }
        documents.sort_by(|left, right| {
            right
                .pinned
                .cmp(&left.pinned)
                .then_with(|| right.updated_at_ms.cmp(&left.updated_at_ms))
                .then_with(|| left.id.cmp(&right.id))
        });
        Ok(documents)
    }

    pub(crate) fn decrypt_document_file(&self, path: &Path) -> Result<ChatDocument> {
        self.read_document_from_path(path)
    }

    pub(crate) fn update_conversation_title(
        &self,
        conversation_id: i64,
        title: &str,
    ) -> Result<()> {
        self.mutate_document(conversation_id, |document| {
            document.title = title.to_string();
        })
    }

    pub(crate) fn set_conversation_pinned(&self, conversation_id: i64, pinned: bool) -> Result<()> {
        self.mutate_document(conversation_id, |document| {
            document.pinned = pinned;
        })
    }

    pub(crate) fn delete_conversation(&self, conversation_id: i64) -> Result<()> {
        if conversation_id < 0 {
            self.ephemeral_documents()?
                .remove(&conversation_id)
                .ok_or_else(|| eyre!("ephemeral conversation is unavailable"))?;
            return Ok(());
        }
        let source = self.chat_path(conversation_id);
        let target = self.trash_path(conversation_id);
        std::fs::create_dir_all(&self.paths.chats_trash_dir)?;
        std::fs::rename(source, target)?;
        Ok(())
    }

    pub(crate) fn archive_legacy_chats(&self, conn: &mut Connection) -> Result<()> {
        if !legacy_chat_tables_exist(conn)? {
            return Ok(());
        }

        let conversations = legacy_conversations(conn)?;
        if conversations.is_empty() {
            return Ok(());
        }

        std::fs::create_dir_all(&self.paths.chats_trash_dir)?;
        let mut archived_paths = Vec::with_capacity(conversations.len());
        for conversation in conversations {
            let path = self.trash_path(conversation.id);
            self.write_document_to_path(&path, &conversation)?;
            let verified = self.read_document_from_path(&path)?;
            if verified.id != conversation.id
                || verified.messages.len() != conversation.messages.len()
            {
                cleanup_paths(&archived_paths);
                let _ = std::fs::remove_file(&path);
                return Err(eyre!("failed to verify archived legacy chat"));
            }
            archived_paths.push(path);
        }

        let transaction = conn.transaction()?;
        transaction.execute("DELETE FROM messages", [])?;
        transaction.execute("DELETE FROM conversations", [])?;
        transaction.commit()?;
        conn.execute_batch("VACUUM")?;
        Ok(())
    }

    fn allocate_id(&self) -> i64 {
        loop {
            let candidate = (rand::random::<u64>() & (i64::MAX as u64)) as i64;
            if candidate == 0 {
                continue;
            }
            if !self.chat_path(candidate).exists() && !self.trash_path(candidate).exists() {
                return candidate;
            }
        }
    }

    pub(crate) fn read_document(&self, conversation_id: i64) -> Result<ChatDocument> {
        if conversation_id < 0 {
            return self
                .ephemeral_documents()?
                .get(&conversation_id)
                .cloned()
                .ok_or_else(|| eyre!("ephemeral conversation is unavailable"));
        }
        self.read_document_from_path(&self.chat_path(conversation_id))
    }

    pub(crate) fn active_chats_dir(&self) -> &Path {
        &self.paths.chats_dir
    }

    pub(crate) fn conversation_exists(&self, conversation_id: i64) -> Result<bool> {
        if conversation_id < 0 {
            return Ok(self.ephemeral_documents()?.contains_key(&conversation_id));
        }
        match self.read_document(conversation_id) {
            Ok(_) => Ok(true),
            Err(error)
                if error
                    .downcast_ref::<crate::storage::crypto::StorageCryptoError>()
                    .is_some_and(|error| {
                        matches!(error, crate::storage::crypto::StorageCryptoError::Io(io)
                            if io.kind() == std::io::ErrorKind::NotFound)
                    }) =>
            {
                Ok(false)
            }
            Err(error) => Err(error),
        }
    }

    fn ephemeral_documents(&self) -> Result<MutexGuard<'_, HashMap<i64, ChatDocument>>> {
        self.ephemeral
            .lock()
            .map_err(|_| eyre!("ephemeral conversation storage lock poisoned"))
    }

    fn mutate_document<T>(
        &self,
        conversation_id: i64,
        mutate: impl FnOnce(&mut ChatDocument) -> T,
    ) -> Result<T> {
        if conversation_id < 0 {
            let mut documents = self.ephemeral_documents()?;
            let document = documents
                .get_mut(&conversation_id)
                .ok_or_else(|| eyre!("ephemeral conversation is unavailable"))?;
            let result = mutate(document);
            document.updated_at_ms = now_ms();
            return Ok(result);
        }
        let mut document = self.read_document(conversation_id)?;
        let result = mutate(&mut document);
        document.updated_at_ms = now_ms();
        self.write_document(&document)?;
        Ok(result)
    }

    fn read_document_from_path(&self, path: &Path) -> Result<ChatDocument> {
        Ok(read_encrypted_document(
            path,
            &self.key,
            CHAT_DOCUMENT_KIND,
        )?)
    }

    fn write_document(&self, document: &ChatDocument) -> Result<()> {
        self.write_document_to_path(&self.chat_path(document.id), document)
    }

    fn write_document_to_path(&self, path: &Path, document: &ChatDocument) -> Result<()> {
        if document.id < 0 {
            return Err(eyre!("cannot persist an ephemeral conversation"));
        }
        Ok(write_encrypted_document(
            path,
            &self.key,
            CHAT_DOCUMENT_KIND,
            document,
        )?)
    }

    fn chat_path(&self, conversation_id: i64) -> PathBuf {
        self.paths
            .chats_dir
            .join(format!("{:016x}.tcui-chat", conversation_id as u64))
    }

    fn trash_path(&self, conversation_id: i64) -> PathBuf {
        self.paths
            .chats_trash_dir
            .join(format!("{:016x}.tcui-chat", conversation_id as u64))
    }
}

fn snapshot_document(
    id: i64,
    tab_id: i64,
    title: &str,
    messages: &[Message],
    parent: Option<ChatLineage>,
) -> ChatDocument {
    let now = now_ms();
    let messages = messages
        .iter()
        .enumerate()
        .map(|(index, message)| {
            let mut copied = message.clone();
            copied.id = Some(index as i64 + 1);
            copied.conversation_id = id;
            copied
        })
        .collect();
    ChatDocument {
        schema_version: CHAT_DOCUMENT_SCHEMA_VERSION,
        id,
        tab_id,
        title: title.to_string(),
        created_at_ms: now,
        updated_at_ms: now,
        pinned: false,
        messages,
        parent,
    }
}

fn legacy_chat_tables_exist(conn: &Connection) -> Result<bool> {
    let count: i64 = conn.query_row(
        "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name IN ('conversations', 'messages')",
        [],
        |row| row.get(0),
    )?;
    Ok(count == 2)
}

fn legacy_conversations(conn: &Connection) -> Result<Vec<ChatDocument>> {
    let mut statement =
        conn.prepare("SELECT id, tab_id, COALESCE(title, 'New Chat') FROM conversations")?;
    let rows = statement.query_map([], |row| {
        Ok((
            row.get::<_, i64>(0)?,
            row.get::<_, i64>(1)?,
            row.get::<_, String>(2)?,
        ))
    })?;

    let mut conversations = Vec::new();
    for row in rows {
        let (conversation_id, tab_id, title) = row?;
        let messages = legacy_messages(conn, conversation_id)?;
        let timestamp = now_ms();
        conversations.push(ChatDocument {
            schema_version: CHAT_DOCUMENT_SCHEMA_VERSION,
            id: conversation_id,
            tab_id,
            title,
            created_at_ms: timestamp,
            updated_at_ms: timestamp,
            pinned: false,
            messages,
            parent: None,
        });
    }
    Ok(conversations)
}

fn legacy_messages(conn: &Connection, conversation_id: i64) -> Result<Vec<Message>> {
    let key = SharedKey::load_or_create_default(&TcuiDataPaths::discover())?.key;
    let mut statement = conn.prepare(
        "SELECT id, conversation_id, role, content, thinking_content, tool_calls, tool_result, tool_source, images, diff_data, token_count
         FROM messages WHERE conversation_id = ?1 ORDER BY id",
    )?;
    let rows = statement.query_map([conversation_id], |row| {
        Ok((
            row.get::<_, i64>(0)?,
            row.get::<_, i64>(1)?,
            row.get::<_, String>(2)?,
            row.get::<_, String>(3)?,
            row.get::<_, Option<String>>(4)?,
            row.get::<_, Option<String>>(5)?,
            row.get::<_, Option<String>>(6)?,
            row.get::<_, Option<String>>(7)?,
            row.get::<_, Option<String>>(8)?,
            row.get::<_, Option<String>>(9)?,
            row.get::<_, Option<i64>>(10)?,
        ))
    })?;

    let mut messages = Vec::new();
    for row in rows {
        let (
            id,
            stored_conversation_id,
            role,
            content,
            thinking_content,
            tool_calls,
            tool_result,
            tool_source,
            images,
            diff_data,
            token_count,
        ) = row?;
        messages.push(Message {
            id: Some(id),
            conversation_id: stored_conversation_id,
            role,
            content: decrypt_shared_text_with_key(&key, &content)?,
            thinking_content: decrypt_optional(&key, thinking_content)?,
            tool_calls: decrypt_optional(&key, tool_calls)?,
            tool_result: decrypt_optional(&key, tool_result)?,
            tool_source: decrypt_optional(&key, tool_source)?,
            images: decrypt_optional(&key, images)?,
            diff_data: decrypt_optional(&key, diff_data)?,
            token_count,
            request: None,
        });
    }

    Ok(messages)
}

fn decrypt_optional(key: &SharedKey, value: Option<String>) -> Result<Option<String>> {
    value
        .map(|stored| decrypt_shared_text_with_key(key, &stored))
        .transpose()
        .map_err(Into::into)
}

fn is_chat_document_path(path: &Path) -> bool {
    path.is_file()
        && path
            .extension()
            .and_then(|extension| extension.to_str())
            .is_some_and(|extension| extension == "tcui-chat")
}

fn cleanup_paths(paths: &[PathBuf]) {
    for path in paths {
        let _ = std::fs::remove_file(path);
    }
}

fn now_ms() -> i64 {
    static LAST_TIMESTAMP_MS: AtomicI64 = AtomicI64::new(0);

    let observed = i64::try_from(
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("system clock before epoch")
            .as_millis(),
    )
    .unwrap_or(i64::MAX);
    let mut previous = LAST_TIMESTAMP_MS.load(Ordering::Relaxed);

    loop {
        let timestamp = observed.max(previous.saturating_add(1));
        match LAST_TIMESTAMP_MS.compare_exchange(
            previous,
            timestamp,
            Ordering::SeqCst,
            Ordering::SeqCst,
        ) {
            Ok(_) => return timestamp,
            Err(current) => previous = current,
        }
    }
}
