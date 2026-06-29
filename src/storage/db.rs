#![allow(dead_code)]
use crate::app::message::Message;
use aes_gcm::{
    aead::{Aead, KeyInit},
    Aes256Gcm,
};
use base64::{engine::general_purpose::STANDARD, Engine as _};
use color_eyre::Result;
use rusqlite::{params, Connection};
use std::path::PathBuf;

pub struct Storage {
    conn: Connection,
    chat_key: [u8; 32],
}

impl Storage {
    pub(crate) fn encrypt_shared_text(plaintext: &str) -> Result<String> {
        if plaintext.is_empty() {
            return Ok(String::new());
        }

        let nonce_bytes = rand::random::<[u8; 12]>();
        let ciphertext = Self::shared_cipher()?
            .encrypt((&nonce_bytes).into(), plaintext.as_bytes())
            .map_err(|_| color_eyre::eyre::eyre!("failed to encrypt local secret"))?;
        Ok(format!(
            "enc:v1:{}:{}",
            STANDARD.encode(nonce_bytes),
            STANDARD.encode(ciphertext)
        ))
    }

    pub(crate) fn decrypt_shared_text(stored: &str) -> Result<String> {
        let Some(encoded) = stored.strip_prefix("enc:v1:") else {
            return Ok(stored.to_string());
        };

        let mut parts = encoded.splitn(2, ':');
        let nonce = parts
            .next()
            .ok_or_else(|| color_eyre::eyre::eyre!("missing local secret nonce"))?;
        let ciphertext = parts
            .next()
            .ok_or_else(|| color_eyre::eyre::eyre!("missing local secret ciphertext"))?;
        let nonce_bytes = STANDARD.decode(nonce)?;
        let ciphertext_bytes = STANDARD.decode(ciphertext)?;
        let plaintext = Self::shared_cipher()?
            .decrypt(nonce_bytes.as_slice().into(), ciphertext_bytes.as_ref())
            .map_err(|_| color_eyre::eyre::eyre!("failed to decrypt local secret"))?;
        Ok(String::from_utf8(plaintext)?)
    }

    pub fn new() -> Result<Self> {
        let db_path = Self::db_path()?;
        if let Some(parent) = db_path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let conn = Connection::open(&db_path)?;
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
        let storage = Self {
            conn,
            chat_key: Self::load_or_create_chat_key()?,
        };
        storage.seed_providers()?;
        if let Ok(config) = crate::config::AppConfig::load() {
            let _ = storage.sync_providers(&config.providers);
        }
        Ok(storage)
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

    fn db_path() -> Result<PathBuf> {
        let dir = dirs::data_dir().unwrap_or_else(|| PathBuf::from("."));
        Ok(dir.join("tcui").join("tcui.db"))
    }

    fn chat_key_path() -> Result<PathBuf> {
        let dir = dirs::data_dir().unwrap_or_else(|| PathBuf::from("."));
        Ok(dir.join("tcui").join("chat.key"))
    }

    fn shared_cipher() -> Result<Aes256Gcm> {
        let key = Self::load_or_create_chat_key()?;
        Aes256Gcm::new_from_slice(&key)
            .map_err(|_| color_eyre::eyre::eyre!("invalid chat encryption key"))
    }

    fn load_or_create_chat_key() -> Result<[u8; 32]> {
        let key_path = Self::chat_key_path()?;
        if let Some(parent) = key_path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        if key_path.exists() {
            let encoded = std::fs::read_to_string(&key_path)?;
            let decoded = STANDARD.decode(encoded.trim())?;
            let key: [u8; 32] = decoded
                .try_into()
                .map_err(|_| color_eyre::eyre::eyre!("invalid chat encryption key length"))?;
            return Ok(key);
        }

        let key = rand::random::<[u8; 32]>();
        std::fs::write(&key_path, STANDARD.encode(key))?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut perms = std::fs::metadata(&key_path)?.permissions();
            perms.set_mode(0o600);
            std::fs::set_permissions(&key_path, perms)?;
        }
        Ok(key)
    }

    fn cipher(&self) -> Result<Aes256Gcm> {
        Aes256Gcm::new_from_slice(&self.chat_key)
            .map_err(|_| color_eyre::eyre::eyre!("invalid chat encryption key"))
    }

    fn encrypt_text(&self, plaintext: &str) -> Result<String> {
        if plaintext.is_empty() {
            return Ok(String::new());
        }

        let nonce_bytes = rand::random::<[u8; 12]>();
        let ciphertext = self
            .cipher()?
            .encrypt((&nonce_bytes).into(), plaintext.as_bytes())
            .map_err(|_| color_eyre::eyre::eyre!("failed to encrypt chat message"))?;
        Ok(format!(
            "enc:v1:{}:{}",
            STANDARD.encode(nonce_bytes),
            STANDARD.encode(ciphertext)
        ))
    }

    fn decrypt_text(&self, stored: &str) -> Result<String> {
        let Some(encoded) = stored.strip_prefix("enc:v1:") else {
            return Ok(stored.to_string());
        };

        let mut parts = encoded.splitn(2, ':');
        let nonce = parts
            .next()
            .ok_or_else(|| color_eyre::eyre::eyre!("missing message nonce"))?;
        let ciphertext = parts
            .next()
            .ok_or_else(|| color_eyre::eyre::eyre!("missing message ciphertext"))?;
        let nonce_bytes = STANDARD.decode(nonce)?;
        let ciphertext_bytes = STANDARD.decode(ciphertext)?;
        let plaintext = self
            .cipher()?
            .decrypt(nonce_bytes.as_slice().into(), ciphertext_bytes.as_ref())
            .map_err(|_| color_eyre::eyre::eyre!("failed to decrypt chat message"))?;
        Ok(String::from_utf8(plaintext)?)
    }

    fn encrypt_optional(&self, value: &Option<String>) -> Result<Option<String>> {
        value
            .as_deref()
            .map(|text| self.encrypt_text(text))
            .transpose()
    }

    fn decrypt_optional(&self, value: Option<String>) -> Result<Option<String>> {
        value.map(|text| self.decrypt_text(&text)).transpose()
    }

    pub fn save_message(&self, msg: &Message) -> Result<i64> {
        let content = self.encrypt_text(&msg.content)?;
        let thinking_content = self.encrypt_optional(&msg.thinking_content)?;
        let tool_calls = self.encrypt_optional(&msg.tool_calls)?;
        let tool_result = self.encrypt_optional(&msg.tool_result)?;
        let tool_source = self.encrypt_optional(&msg.tool_source)?;
        let images = self.encrypt_optional(&msg.images)?;
        let diff_data = self.encrypt_optional(&msg.diff_data)?;

        self.conn.execute(
            "INSERT INTO messages (conversation_id, role, content, thinking_content, tool_calls, tool_result, tool_source, images, diff_data, token_count)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
            params![
                msg.conversation_id,
                msg.role,
                content,
                thinking_content,
                tool_calls,
                tool_result,
                tool_source,
                images,
                diff_data,
                msg.token_count,
            ],
        )?;
        Ok(self.conn.last_insert_rowid())
    }

    pub fn get_messages(&self, conversation_id: i64) -> Result<Vec<Message>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, conversation_id, role, content, thinking_content, tool_calls, tool_result, tool_source, images, diff_data, token_count
             FROM messages WHERE conversation_id = ?1"
        )?;

        let rows = stmt.query_map([conversation_id], |row| {
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
                conversation_id,
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
                conversation_id,
                role,
                content: self.decrypt_text(&content)?,
                thinking_content: self.decrypt_optional(thinking_content)?,
                tool_calls: self.decrypt_optional(tool_calls)?,
                tool_result: self.decrypt_optional(tool_result)?,
                tool_source: self.decrypt_optional(tool_source)?,
                images: self.decrypt_optional(images)?,
                diff_data: self.decrypt_optional(diff_data)?,
                token_count,
            });
        }

        Ok(messages)
    }

    pub fn create_conversation(&self, tab_id: i64) -> Result<i64> {
        self.conn.execute(
            "INSERT INTO conversations (tab_id) VALUES (?1)",
            params![tab_id],
        )?;
        Ok(self.conn.last_insert_rowid())
    }

    pub fn get_conversations(&self, tab_id: i64) -> Result<Vec<ConversationEntry>> {
        let mut stmt = self
            .conn
            .prepare("SELECT id, title FROM conversations WHERE tab_id = ?1")?;

        let entries = stmt
            .query_map([tab_id], |row| {
                Ok(ConversationEntry {
                    id: row.get(0)?,
                    title: row.get(1)?,
                    created_at: String::new(),
                })
            })?
            .filter_map(|r| r.ok())
            .collect();

        Ok(entries)
    }

    pub fn update_conversation_title(&self, conv_id: i64, title: &str) -> Result<()> {
        self.conn.execute(
            "UPDATE conversations SET title = ?1 WHERE id = ?2",
            params![title, conv_id],
        )?;
        Ok(())
    }

    pub fn delete_conversation(&self, conv_id: i64) -> Result<()> {
        self.conn.execute(
            "DELETE FROM messages WHERE conversation_id = ?1",
            params![conv_id],
        )?;
        self.conn
            .execute("DELETE FROM conversations WHERE id = ?1", params![conv_id])?;
        Ok(())
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

    pub fn get_providers(&self) -> Result<Vec<(String, String, String, String, String)>> {
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
        rows.filter_map(|r| r.ok())
            .collect::<Vec<_>>()
            .into_iter()
            .map(|r| Ok(r))
            .collect()
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

    pub fn save_models(
        &self,
        provider: &str,
        models: &[(String, Option<f64>, Option<f64>)],
    ) -> Result<()> {
        if models.is_empty() {
            return Ok(());
        }

        let tx = self.conn.unchecked_transaction()?;
        self.conn
            .execute("DELETE FROM models WHERE provider = ?1", params![provider])?;
        for (model_id, input_price, output_price) in models {
            self.conn.execute(
                "INSERT INTO models (provider, model_id, input_price, output_price, fetched_at)
                 VALUES (?1, ?2, ?3, ?4, datetime('now'))",
                params![provider, model_id, input_price, output_price],
            )?;
        }
        tx.commit()?;
        Ok(())
    }

    pub fn get_models(&self, provider: &str) -> Result<Vec<(String, Option<f64>, Option<f64>)>> {
        let mut stmt = self.conn.prepare(
            "SELECT model_id, input_price, output_price FROM models WHERE provider = ?1 ORDER BY model_id"
        )?;
        let rows = stmt.query_map([provider], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, Option<f64>>(1)?,
                row.get::<_, Option<f64>>(2)?,
            ))
        })?;
        rows.filter_map(|r| r.ok())
            .collect::<Vec<_>>()
            .into_iter()
            .map(|r| Ok(r))
            .collect()
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
}

#[cfg(test)]
mod tests {
    use super::*;
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
        storage.save_message(&message).expect("save message");

        let raw_content: String = storage
            .conn
            .query_row("SELECT content FROM messages LIMIT 1", [], |row| row.get(0))
            .expect("read raw content");
        assert!(
            raw_content.starts_with("enc:v1:"),
            "expected encrypted message content"
        );
        assert!(
            !raw_content.contains("hello world"),
            "plaintext leaked into storage"
        );

        let messages = storage
            .get_messages(conversation_id)
            .expect("load decrypted messages");
        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0].content, "hello world");
        assert_eq!(
            messages[0].thinking_content.as_deref(),
            Some("private chain")
        );

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
}
