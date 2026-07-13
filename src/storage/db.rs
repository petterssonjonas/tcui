#![allow(dead_code)]
use crate::app::message::Message;
use color_eyre::Result;
use rusqlite::{Connection, params, types::Type};

use crate::storage::chat_store::{ChatDocument, ChatStore};
use crate::storage::crypto::{
    SharedKey, decrypt_shared_text_with_key, encrypt_shared_text_with_key,
};
use crate::storage::paths::TcuiDataPaths;

pub type ProviderRow = (String, String, String, String, String);
pub type ModelRow = (
    String,
    Option<f64>,
    Option<f64>,
    Option<u32>,
    Option<String>,
    Vec<String>,
);

pub struct Storage {
    conn: Connection,
    chat_store: ChatStore,
    created_default_key: bool,
}

impl Storage {
    pub(crate) fn encrypt_shared_text(plaintext: &str) -> Result<String> {
        let shared_key = SharedKey::load_or_create_default(&TcuiDataPaths::discover())?;
        Ok(encrypt_shared_text_with_key(&shared_key.key, plaintext)?)
    }

    pub(crate) fn decrypt_shared_text(stored: &str) -> Result<String> {
        let shared_key = SharedKey::load_or_create_default(&TcuiDataPaths::discover())?;
        Ok(decrypt_shared_text_with_key(&shared_key.key, stored)?)
    }

    pub fn new() -> Result<Self> {
        let paths = TcuiDataPaths::discover();
        paths.ensure_layout()?;
        let conn = Connection::open(&paths.database)?;
        let sql = include_str!("../../migrations/init.sql");
        conn.execute_batch(sql)?;
        let _ = conn.execute(
            "ALTER TABLE providers ADD COLUMN backend_type TEXT NOT NULL DEFAULT 'openai'",
            [],
        );
        let _ = conn.execute(
            "ALTER TABLE providers ADD COLUMN auth_type TEXT NOT NULL DEFAULT 'api_key'",
            [],
        );
        let _ = conn.execute("ALTER TABLE models ADD COLUMN context_window INTEGER", []);
        let _ = conn.execute(
            "ALTER TABLE models ADD COLUMN default_reasoning_effort TEXT",
            [],
        );
        let _ = conn.execute(
            "ALTER TABLE models ADD COLUMN supported_reasoning_efforts TEXT NOT NULL DEFAULT '[]'",
            [],
        );
        let shared_key = SharedKey::load_or_create_default(&paths)?;
        Self::from_parts(conn, paths, shared_key.key, shared_key.created_default_key)
    }

    pub fn new_with_key(shared_key: SharedKey) -> Result<Self> {
        let paths = TcuiDataPaths::discover();
        paths.ensure_layout()?;
        let conn = Connection::open(&paths.database)?;
        let sql = include_str!("../../migrations/init.sql");
        conn.execute_batch(sql)?;
        let _ = conn.execute(
            "ALTER TABLE providers ADD COLUMN backend_type TEXT NOT NULL DEFAULT 'openai'",
            [],
        );
        let _ = conn.execute(
            "ALTER TABLE providers ADD COLUMN auth_type TEXT NOT NULL DEFAULT 'api_key'",
            [],
        );
        let _ = conn.execute("ALTER TABLE models ADD COLUMN context_window INTEGER", []);
        let _ = conn.execute(
            "ALTER TABLE models ADD COLUMN default_reasoning_effort TEXT",
            [],
        );
        let _ = conn.execute(
            "ALTER TABLE models ADD COLUMN supported_reasoning_efforts TEXT NOT NULL DEFAULT '[]'",
            [],
        );
        Self::from_parts(conn, paths, shared_key, false)
    }

    fn from_parts(
        mut conn: Connection,
        paths: TcuiDataPaths,
        shared_key: SharedKey,
        created_default_key: bool,
    ) -> Result<Self> {
        let chat_store = ChatStore::new(paths.clone(), shared_key.clone());
        chat_store.archive_legacy_chats(&mut conn)?;
        let storage = Self {
            conn,
            chat_store,
            created_default_key,
        };
        storage.seed_providers()?;
        if let Ok(config) = crate::config::AppConfig::load() {
            let _ = storage.sync_providers(&config.providers);
        }
        Ok(storage)
    }

    pub fn created_default_key(&self) -> bool {
        self.created_default_key
    }

    pub fn chat_key_path() -> std::path::PathBuf {
        TcuiDataPaths::discover().chat_key
    }

    fn seed_providers(&self) -> Result<()> {
        for (name, endpoint, env_var, backend_type, auth_type) in
            crate::config::AppConfig::default().provider_entries()
        {
            self.conn.execute(
                "INSERT OR IGNORE INTO providers (name, endpoint, env_var, backend_type, auth_type) VALUES (?1, ?2, ?3, ?4, ?5)",
                params![name, endpoint, env_var, backend_type, auth_type],
            )?;
        }
        Ok(())
    }

    pub fn save_message(&self, msg: &Message) -> Result<i64> {
        self.chat_store.save_message(msg)
    }

    pub fn get_messages(&self, conversation_id: i64) -> Result<Vec<Message>> {
        self.chat_store.get_messages(conversation_id)
    }

    pub fn replace_messages(&self, conversation_id: i64, messages: &[Message]) -> Result<()> {
        self.chat_store.replace_messages(conversation_id, messages)
    }

    pub fn create_conversation(&self, tab_id: i64) -> Result<i64> {
        self.chat_store.create_conversation(tab_id)
    }

    pub fn get_conversations(&self, tab_id: i64) -> Result<Vec<ConversationEntry>> {
        self.chat_store.get_conversations(tab_id)
    }

    pub fn get_conversations_with_warnings(
        &self,
        tab_id: i64,
    ) -> Result<(Vec<ConversationEntry>, usize)> {
        self.chat_store.get_conversations_with_warnings(tab_id)
    }

    pub fn list_all_chat_documents(&self) -> Result<Vec<ChatDocument>> {
        self.chat_store.list_all_documents()
    }

    pub fn decrypt_chat_file(&self, path: &std::path::Path) -> Result<ChatDocument> {
        self.chat_store.decrypt_document_file(path)
    }

    pub fn update_conversation_title(&self, conv_id: i64, title: &str) -> Result<()> {
        self.chat_store.update_conversation_title(conv_id, title)
    }

    pub fn set_conversation_pinned(&self, conv_id: i64, pinned: bool) -> Result<()> {
        self.chat_store.set_conversation_pinned(conv_id, pinned)
    }

    pub fn delete_conversation(&self, conv_id: i64) -> Result<()> {
        self.chat_store.delete_conversation(conv_id)
    }

    pub fn get_setting(&self, key: &str) -> Result<Option<String>> {
        let mut stmt = self
            .conn
            .prepare("SELECT value FROM settings WHERE key = ?1")?;
        let mut rows = stmt.query([key])?;
        if let Some(row) = rows.next()? {
            Ok(Some(row.get(0)?))
        } else {
            Ok(None)
        }
    }

    pub fn set_setting(&self, key: &str, value: &str) -> Result<()> {
        self.conn.execute(
            "INSERT INTO settings (key, value) VALUES (?1, ?2) ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            params![key, value],
        )?;
        Ok(())
    }

    pub fn delete_setting(&self, key: &str) -> Result<()> {
        self.conn
            .execute("DELETE FROM settings WHERE key = ?1", params![key])?;
        Ok(())
    }

    pub fn get_all_settings(&self) -> Result<std::collections::HashMap<String, String>> {
        let mut stmt = self.conn.prepare("SELECT key, value FROM settings")?;
        let rows = stmt.query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })?;
        let mut map = std::collections::HashMap::new();
        for row in rows {
            let (k, v) = row?;
            map.insert(k, v);
        }
        Ok(map)
    }

    pub fn get_providers(&self) -> Result<Vec<ProviderRow>> {
        self.migrate_legacy_codex_provider()?;
        let mut stmt = self.conn.prepare(
            "SELECT name, endpoint, env_var, backend_type, auth_type FROM providers ORDER BY name",
        )?;
        let rows = stmt.query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, String>(4)?,
            ))
        })?;
        rows.filter_map(|r| r.ok()).map(Ok).collect()
    }

    fn migrate_legacy_codex_provider(&self) -> Result<()> {
        let migrated = self.conn.execute(
            "UPDATE providers
             SET endpoint = ?1, backend_type = ?2
             WHERE name = ?3 AND instr(endpoint, ?4) = 1 AND backend_type = ?5",
            params![
                "https://chatgpt.com/backend-api/codex",
                "codex",
                "Codex",
                "https://api.openai.com",
                "openai",
            ],
        )?;
        if migrated > 0 {
            crate::diagnostics::provider_migration("Codex");
        }
        Ok(())
    }

    pub fn get_provider_endpoint(&self, name: &str) -> Result<Option<String>> {
        let mut stmt = self
            .conn
            .prepare("SELECT endpoint FROM providers WHERE name = ?1")?;
        let mut rows = stmt.query([name])?;
        if let Some(row) = rows.next()? {
            Ok(Some(row.get(0)?))
        } else {
            Ok(None)
        }
    }

    pub fn get_provider_env_var(&self, name: &str) -> Result<Option<String>> {
        let mut stmt = self
            .conn
            .prepare("SELECT env_var FROM providers WHERE name = ?1")?;
        let mut rows = stmt.query([name])?;
        if let Some(row) = rows.next()? {
            Ok(Some(row.get(0)?))
        } else {
            Ok(None)
        }
    }

    pub fn save_models(&self, provider: &str, models: &[ModelRow]) -> Result<()> {
        if models.is_empty() {
            return Ok(());
        }

        let tx = self.conn.unchecked_transaction()?;
        self.conn
            .execute("DELETE FROM models WHERE provider = ?1", params![provider])?;
        for (
            model_id,
            input_price,
            output_price,
            context_window,
            default_reasoning_effort,
            supported_reasoning_efforts,
        ) in models
        {
            let supported_reasoning_efforts = serde_json::to_string(supported_reasoning_efforts)?;
            self.conn.execute(
                "INSERT INTO models (
                    provider,
                    model_id,
                    input_price,
                    output_price,
                    context_window,
                    default_reasoning_effort,
                    supported_reasoning_efforts,
                    fetched_at
                 ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, datetime('now'))",
                params![
                    provider,
                    model_id,
                    input_price,
                    output_price,
                    context_window,
                    default_reasoning_effort,
                    supported_reasoning_efforts
                ],
            )?;
        }
        tx.commit()?;
        Ok(())
    }

    pub fn get_models(&self, provider: &str) -> Result<Vec<ModelRow>> {
        let mut stmt = self.conn.prepare(
            "SELECT
                model_id,
                input_price,
                output_price,
                context_window,
                default_reasoning_effort,
                supported_reasoning_efforts
             FROM models
             WHERE provider = ?1
             ORDER BY model_id",
        )?;
        let rows = stmt.query_map([provider], |row| {
            let encoded_efforts = row.get::<_, String>(5)?;
            let supported_reasoning_efforts =
                serde_json::from_str(&encoded_efforts).map_err(|error| {
                    rusqlite::Error::FromSqlConversionFailure(5, Type::Text, Box::new(error))
                })?;
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, Option<f64>>(1)?,
                row.get::<_, Option<f64>>(2)?,
                row.get::<_, Option<u32>>(3)?,
                row.get::<_, Option<String>>(4)?,
                supported_reasoning_efforts,
            ))
        })?;
        rows.filter_map(|r| r.ok()).map(Ok).collect()
    }

    pub fn get_provider_models_fetched_at(&self, provider: &str) -> Result<Option<String>> {
        let mut stmt = self
            .conn
            .prepare("SELECT fetched_at FROM models WHERE provider = ?1 LIMIT 1")?;
        let mut rows = stmt.query([provider])?;
        if let Some(row) = rows.next()? {
            Ok(Some(row.get(0)?))
        } else {
            Ok(None)
        }
    }

    pub fn update_provider(
        &self,
        name: &str,
        endpoint: &str,
        backend_type: &str,
        auth_type: &str,
    ) -> Result<()> {
        self.conn.execute(
            "UPDATE providers SET endpoint = ?1, backend_type = ?2, auth_type = ?3 WHERE name = ?4",
            params![endpoint, backend_type, auth_type, name],
        )?;
        Ok(())
    }

    pub fn delete_provider(&self, name: &str) -> Result<()> {
        self.conn
            .execute("DELETE FROM providers WHERE name = ?1", params![name])?;
        self.conn
            .execute("DELETE FROM models WHERE provider = ?1", params![name])?;
        let settings_key = format!("api_key_{}", name.to_lowercase());
        let _ = self.delete_setting(&settings_key);
        Ok(())
    }

    pub fn add_provider(
        &self,
        name: &str,
        endpoint: &str,
        env_var: &str,
        backend_type: &str,
        auth_type: &str,
    ) -> Result<()> {
        self.conn.execute(
            "INSERT INTO providers (name, endpoint, env_var, backend_type, auth_type) VALUES (?1, ?2, ?3, ?4, ?5)",
            params![name, endpoint, env_var, backend_type, auth_type],
        )?;
        Ok(())
    }

    pub fn sync_providers(&self, providers: &[crate::config::ProviderConfig]) -> Result<()> {
        let tx = self.conn.unchecked_transaction()?;
        for provider in providers {
            self.conn.execute(
                "INSERT INTO providers (name, endpoint, env_var, backend_type, auth_type)
                 VALUES (?1, ?2, ?3, ?4, ?5)
                 ON CONFLICT(name) DO UPDATE SET
                    endpoint = excluded.endpoint,
                    env_var = excluded.env_var,
                    backend_type = excluded.backend_type,
                    auth_type = excluded.auth_type",
                params![
                    provider.name,
                    provider.endpoint,
                    provider.env_var,
                    provider.backend_type,
                    provider.auth_type,
                ],
            )?;
        }

        let configured_names: std::collections::HashSet<&str> = providers
            .iter()
            .map(|provider| provider.name.as_str())
            .collect();
        for (name, _, _, _, _) in self.get_providers()? {
            if !configured_names.contains(name.as_str()) {
                self.conn
                    .execute("DELETE FROM providers WHERE name = ?1", params![name])?;
                self.conn
                    .execute("DELETE FROM models WHERE provider = ?1", params![name])?;
            }
        }
        tx.commit()?;
        Ok(())
    }
}

impl Default for Storage {
    fn default() -> Self {
        Self::new().expect("Failed to initialize storage")
    }
}

pub struct ConversationEntry {
    pub id: i64,
    pub title: String,
    pub created_at: String,
    pub updated_at_ms: i64,
    pub pinned: bool,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::chat_store::ChatDocument;
    use crate::storage::crypto::{SharedKey, read_encrypted_document};
    use std::path::PathBuf;
    use std::sync::Mutex;

    fn env_lock() -> &'static Mutex<()> {
        crate::test_support::env_lock()
    }

    fn unique_temp_dir(label: &str) -> PathBuf {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("system clock before epoch")
            .as_nanos();
        std::env::temp_dir().join(format!("tcui-{label}-{}-{nanos}", std::process::id()))
    }

    #[test]
    fn messages_are_encrypted_at_rest_and_decrypted_on_read() {
        let _guard = env_lock().lock().expect("env lock poisoned");
        let root = unique_temp_dir("storage-encryption");
        let data_home = root.join("data-home");
        std::fs::create_dir_all(&data_home).expect("create data dir");
        std::env::set_var("XDG_DATA_HOME", &data_home);

        let storage = Storage::new().expect("create storage");
        let conversation_id = storage.create_conversation(0).expect("create conversation");
        let mut message = Message::new(
            conversation_id,
            "user".to_string(),
            "hello world".to_string(),
        );
        message.thinking_content = Some("private chain".to_string());
        let message_id = storage.save_message(&message).expect("save message");

        let raw_content = std::fs::read_to_string(chat_path(&data_home, conversation_id))
            .expect("read chat file");
        assert!(
            raw_content.starts_with("enc:v1:"),
            "expected encrypted chat document"
        );
        assert!(
            !raw_content.contains("hello world"),
            "plaintext leaked into storage"
        );
        assert!(
            !raw_content.contains("private chain"),
            "thinking content leaked into storage"
        );

        let messages = storage
            .get_messages(conversation_id)
            .expect("load decrypted messages");
        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0].id, Some(message_id));
        assert_eq!(messages[0].content, "hello world");
        assert_eq!(
            messages[0].thinking_content.as_deref(),
            Some("private chain")
        );

        std::fs::remove_dir_all(&root).expect("cleanup temp dir");
        std::env::remove_var("XDG_DATA_HOME");
    }

    #[test]
    fn chat_delete_moves_encrypted_document_to_trash() {
        let _guard = env_lock().lock().expect("env lock poisoned");
        let root = unique_temp_dir("storage-delete-chat");
        let data_home = root.join("data-home");
        std::fs::create_dir_all(&data_home).expect("create data dir");
        std::env::set_var("XDG_DATA_HOME", &data_home);

        let storage = Storage::new().expect("create storage");
        let conversation_id = storage.create_conversation(0).expect("create conversation");
        storage
            .save_message(&Message::new(
                conversation_id,
                "user".to_string(),
                "hello world".to_string(),
            ))
            .expect("save message");
        storage
            .delete_conversation(conversation_id)
            .expect("delete conversation");

        assert!(!chat_path(&data_home, conversation_id).exists());
        assert!(trash_chat_path(&data_home, conversation_id).exists());
        assert!(
            storage
                .get_conversations(0)
                .expect("list conversations")
                .is_empty()
        );

        std::fs::remove_dir_all(&root).expect("cleanup temp dir");
        std::env::remove_var("XDG_DATA_HOME");
    }

    #[test]
    fn pinned_chats_sort_ahead_of_recent_unpinned_chats() {
        let _guard = env_lock().lock().expect("env lock poisoned");
        let root = unique_temp_dir("storage-pinned-sort");
        let data_home = root.join("data-home");
        std::fs::create_dir_all(&data_home).expect("create data dir");
        std::env::set_var("XDG_DATA_HOME", &data_home);

        let storage = Storage::new().expect("create storage");
        let first = storage
            .create_conversation(0)
            .expect("create first conversation");
        storage
            .update_conversation_title(first, "Pinned first")
            .expect("update first title");
        let second = storage
            .create_conversation(0)
            .expect("create second conversation");
        storage
            .update_conversation_title(second, "Newest second")
            .expect("update second title");
        storage
            .set_conversation_pinned(first, true)
            .expect("pin first conversation");

        let conversations = storage.get_conversations(0).expect("list conversations");
        assert_eq!(conversations.len(), 2);
        assert_eq!(conversations[0].id, first);
        assert!(conversations[0].pinned);
        assert_eq!(conversations[1].id, second);
        assert!(!conversations[1].pinned);

        std::fs::remove_dir_all(&root).expect("cleanup temp dir");
        std::env::remove_var("XDG_DATA_HOME");
    }

    #[test]
    fn legacy_sqlite_chats_are_archived_to_encrypted_trash() {
        let _guard = env_lock().lock().expect("env lock poisoned");
        let root = unique_temp_dir("storage-legacy-archive");
        let data_home = root.join("data-home");
        std::fs::create_dir_all(&data_home).expect("create data dir");
        std::env::set_var("XDG_DATA_HOME", &data_home);

        let encrypted_content =
            Storage::encrypt_shared_text("hello from sqlite").expect("encrypt legacy content");
        let encrypted_thinking =
            Storage::encrypt_shared_text("sqlite thinking").expect("encrypt thinking");
        let database_path = data_home.join("tcui").join("tcui.db");
        if let Some(parent) = database_path.parent() {
            std::fs::create_dir_all(parent).expect("create db parent");
        }
        let connection = Connection::open(&database_path).expect("open sqlite db");
        connection
            .execute_batch(
                "CREATE TABLE conversations (
                    id INTEGER PRIMARY KEY,
                    tab_id INTEGER NOT NULL,
                    title TEXT
                );
                CREATE TABLE messages (
                    id INTEGER PRIMARY KEY,
                    conversation_id INTEGER NOT NULL,
                    role TEXT NOT NULL,
                    content TEXT NOT NULL,
                    thinking_content TEXT,
                    tool_calls TEXT,
                    tool_result TEXT,
                    tool_source TEXT,
                    images TEXT,
                    diff_data TEXT,
                    token_count INTEGER
                );",
            )
            .expect("create legacy tables");
        connection
            .execute(
                "INSERT INTO conversations (id, tab_id, title) VALUES (?1, ?2, ?3)",
                params![42_i64, 7_i64, "Legacy chat"],
            )
            .expect("insert legacy conversation");
        connection
            .execute(
                "INSERT INTO messages (id, conversation_id, role, content, thinking_content, tool_calls, tool_result, tool_source, images, diff_data, token_count)
                 VALUES (?1, ?2, ?3, ?4, ?5, NULL, NULL, NULL, NULL, NULL, NULL)",
                params![9_i64, 42_i64, "user", encrypted_content, encrypted_thinking],
            )
            .expect("insert legacy message");
        drop(connection);

        let storage = Storage::new().expect("create storage");
        assert!(
            storage
                .get_conversations(7)
                .expect("list active conversations")
                .is_empty()
        );

        let archived_path = trash_chat_path(&data_home, 42);
        assert!(archived_path.exists(), "legacy chat should be archived");
        let key = SharedKey::load_or_create_default(&TcuiDataPaths::discover())
            .expect("load shared key")
            .key;
        let archived: ChatDocument =
            read_encrypted_document(&archived_path, &key, "chat").expect("read archived chat");
        assert_eq!(archived.id, 42);
        assert_eq!(archived.title, "Legacy chat");
        assert_eq!(archived.messages.len(), 1);
        assert_eq!(archived.messages[0].content, "hello from sqlite");
        assert_eq!(
            archived.messages[0].thinking_content.as_deref(),
            Some("sqlite thinking")
        );

        let reopened = Connection::open(&database_path).expect("reopen sqlite db");
        let conversation_count: i64 = reopened
            .query_row("SELECT COUNT(*) FROM conversations", [], |row| row.get(0))
            .expect("count conversations");
        let message_count: i64 = reopened
            .query_row("SELECT COUNT(*) FROM messages", [], |row| row.get(0))
            .expect("count messages");
        assert_eq!(conversation_count, 0);
        assert_eq!(message_count, 0);

        std::fs::remove_dir_all(&root).expect("cleanup temp dir");
        std::env::remove_var("XDG_DATA_HOME");
    }

    #[cfg(feature = "memory")]
    #[test]
    fn memory_activity_survives_conversation_reload() {
        let _guard = env_lock().lock().expect("env lock poisoned");
        let root = unique_temp_dir("memory-activity-reload");
        let data_home = root.join("data-home");
        std::fs::create_dir_all(&data_home).expect("create data dir");
        std::env::set_var("XDG_DATA_HOME", &data_home);
        let storage = Storage::new().expect("create storage");
        let conversation_id = storage.create_conversation(0).expect("create conversation");
        let mut message = Message::new(
            conversation_id,
            "assistant".to_string(),
            "Answer".to_string(),
        );
        crate::memory::set_activities(
            &mut message,
            &[crate::memory::MemoryActivity::Saved {
                title: "Preference".to_string(),
                path: "preference.md".to_string(),
            }],
        )
        .expect("memory activity");

        storage.save_message(&message).expect("save message");
        let messages = storage
            .get_messages(conversation_id)
            .expect("reload messages");

        assert_eq!(
            crate::memory::activities(&messages[0]),
            [crate::memory::MemoryActivity::Saved {
                title: "Preference".to_string(),
                path: "preference.md".to_string(),
            }]
        );
        std::fs::remove_dir_all(root).expect("cleanup temp dir");
        std::env::remove_var("XDG_DATA_HOME");
    }

    #[test]
    fn provider_load_migrates_legacy_codex_configuration_once() {
        let (storage, root) = provider_storage("codex-provider-migration");
        storage
            .update_provider("Codex", "https://api.openai.com/v1", "openai", "oauth")
            .expect("seed legacy Codex provider");

        let providers = storage.get_providers().expect("load providers");
        let codex = providers
            .iter()
            .find(|(name, _, _, _, _)| name == "Codex")
            .expect("Codex provider");

        assert_eq!(
            codex,
            &(
                "Codex".to_owned(),
                "https://chatgpt.com/backend-api/codex".to_owned(),
                "CODEX_API_KEY".to_owned(),
                "codex".to_owned(),
                "oauth".to_owned(),
            )
        );
        let changes_after_first_load = storage.conn.total_changes();
        storage.get_providers().expect("reload providers");
        assert_eq!(storage.conn.total_changes(), changes_after_first_load);

        std::fs::remove_dir_all(root).expect("cleanup temp dir");
    }

    #[test]
    fn provider_load_preserves_custom_codex_endpoint() {
        let (storage, root) = provider_storage("codex-provider-custom-endpoint");
        storage
            .update_provider(
                "Codex",
                "https://gateway.example.test/codex",
                "openai",
                "oauth",
            )
            .expect("seed customized Codex provider");

        let providers = storage.get_providers().expect("load providers");
        let codex = providers
            .iter()
            .find(|(name, _, _, _, _)| name == "Codex")
            .expect("Codex provider");

        assert_eq!(codex.1, "https://gateway.example.test/codex");
        assert_eq!(codex.3, "openai");

        std::fs::remove_dir_all(root).expect("cleanup temp dir");
    }

    #[test]
    fn provider_load_preserves_noncanonical_codex_name() {
        let (storage, root) = provider_storage("codex-provider-noncanonical-name");
        storage
            .add_provider(
                "codex",
                "https://api.openai.com/v1",
                "CUSTOM_CODEX_KEY",
                "openai",
                "api_key",
            )
            .expect("seed noncanonical Codex provider");

        let providers = storage.get_providers().expect("load providers");
        let custom = providers
            .iter()
            .find(|(name, _, _, _, _)| name == "codex")
            .expect("noncanonical Codex provider");

        assert_eq!(custom.1, "https://api.openai.com/v1");
        assert_eq!(custom.3, "openai");

        std::fs::remove_dir_all(root).expect("cleanup temp dir");
    }

    #[test]
    fn model_cache_round_trips_reasoning_metadata() -> Result<(), Box<dyn std::error::Error>> {
        let (storage, root) = provider_storage("model-reasoning-metadata");
        let models = vec![(
            "gpt-5.6-sol".to_string(),
            None,
            None,
            Some(272_000),
            Some("medium".to_string()),
            vec!["low".to_string(), "medium".to_string(), "high".to_string()],
        )];

        storage.save_models("Codex", &models)?;
        let loaded = storage.get_models("Codex")?;

        assert_eq!(loaded, models);
        std::fs::remove_dir_all(root)?;
        Ok(())
    }

    fn provider_storage(label: &str) -> (Storage, PathBuf) {
        let root = unique_temp_dir(label);
        let paths = TcuiDataPaths::from_root(root.clone());
        paths.ensure_layout().expect("create provider test layout");
        let shared_key = SharedKey::load_or_create_default(&paths)
            .expect("create provider test key")
            .key;
        let connection = Connection::open_in_memory().expect("open provider test database");
        connection
            .execute_batch(include_str!("../../migrations/init.sql"))
            .expect("initialize provider test database");
        let storage = Storage {
            conn: connection,
            chat_store: ChatStore::new(paths, shared_key),
            created_default_key: false,
        };
        storage.seed_providers().expect("seed providers");
        (storage, root)
    }

    fn chat_path(data_home: &PathBuf, conversation_id: i64) -> PathBuf {
        data_home
            .join("tcui")
            .join("chats")
            .join(format!("{:016x}.tcui-chat", conversation_id as u64))
    }

    fn trash_chat_path(data_home: &PathBuf, conversation_id: i64) -> PathBuf {
        data_home
            .join("tcui")
            .join("chats")
            .join(".trash")
            .join(format!("{:016x}.tcui-chat", conversation_id as u64))
    }
}
