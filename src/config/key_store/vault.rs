use super::KeyStoreError;
use crate::storage::paths::{set_unix_mode, TcuiDataPaths};
use aes_gcm::{
    aead::{Aead, KeyInit, Payload},
    Aes256Gcm,
};
use argon2::{Algorithm, Argon2, Params, Version};
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{LazyLock, Mutex, MutexGuard};
use zeroize::Zeroizing;

type Result<T, E = KeyStoreError> = std::result::Result<T, E>;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum CredentialStorage {
    Encrypted,
    Plaintext,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SecretKind {
    Password,
    SshKey,
}

impl SecretKind {
    pub(super) fn record_kind(self) -> &'static str {
        match self {
            Self::Password => "machine_password",
            Self::SshKey => "ssh_key",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SecretEntry {
    pub id: String,
    pub label: String,
    pub kind: SecretKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VaultStatus {
    pub initialized: bool,
    pub unlocked: bool,
    pub storage: CredentialStorage,
    pub legacy_present: bool,
}

pub(super) struct Record {
    pub kind: String,
    pub id: String,
    pub label: String,
    storage: CredentialStorage,
    nonce: Vec<u8>,
    payload: Zeroizing<Vec<u8>>,
}

pub(super) struct Vault {
    pub conn: Connection,
    path: PathBuf,
}

type UnlockKeys = HashMap<PathBuf, Zeroizing<[u8; 32]>>;
pub(super) fn session() -> Result<MutexGuard<'static, UnlockKeys>> {
    static KEYS: LazyLock<Mutex<UnlockKeys>> = LazyLock::new(|| Mutex::new(HashMap::new()));
    KEYS.lock().map_err(|_| KeyStoreError::KeyAccess)
}

impl Vault {
    pub fn open() -> Result<Self> {
        let paths = TcuiDataPaths::discover();
        super::path_security::open_parent(&paths.root, true)?
            .ok_or(KeyStoreError::CreateDirectory)?;
        set_unix_mode(&paths.root, 0o700).map_err(|_| KeyStoreError::UnsafePath)?;
        match std::fs::symlink_metadata(&paths.database) {
            Ok(metadata) if !metadata.is_file() || metadata.file_type().is_symlink() => {
                return Err(KeyStoreError::UnsafePath)
            }
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(_) => return Err(KeyStoreError::Read),
        }
        let conn = Connection::open_with_flags(
            &paths.database,
            rusqlite::OpenFlags::SQLITE_OPEN_READ_WRITE
                | rusqlite::OpenFlags::SQLITE_OPEN_CREATE
                | rusqlite::OpenFlags::SQLITE_OPEN_NOFOLLOW,
        )
        .map_err(|_| KeyStoreError::Read)?;
        set_unix_mode(&paths.database, 0o600).map_err(|_| KeyStoreError::UnsafePath)?;
        conn.busy_timeout(std::time::Duration::from_secs(5))
            .map_err(|_| KeyStoreError::Read)?;
        conn.execute_batch("PRAGMA secure_delete=ON")
            .map_err(|_| KeyStoreError::Write)?;
        let exists: bool = conn.query_row(
            "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name='tcui_credential_vault')",
            [], |row| row.get(0),
        ).map_err(|_| KeyStoreError::Read)?;
        if !exists {
            conn.execute_batch("BEGIN IMMEDIATE;
                CREATE TABLE IF NOT EXISTS tcui_credential_vault (
                    singleton INTEGER PRIMARY KEY CHECK(singleton=1),
                    version INTEGER NOT NULL CHECK(version=1),
                    preference INTEGER NOT NULL CHECK(preference IN (0,1)),
                    salt BLOB, verifier_nonce BLOB, verifier BLOB
                );
                INSERT OR IGNORE INTO tcui_credential_vault(singleton,version,preference) VALUES(1,1,1);
                CREATE TABLE IF NOT EXISTS tcui_credentials (
                    kind TEXT NOT NULL, id TEXT NOT NULL, label TEXT NOT NULL,
                    encrypted INTEGER NOT NULL CHECK(encrypted IN (0,1)),
                    nonce BLOB NOT NULL, payload BLOB NOT NULL,
                    PRIMARY KEY(kind,id)
                );
                COMMIT;").map_err(|_| KeyStoreError::Write)?;
        }
        Self::validate_metadata(&conn)?;
        Ok(Self {
            conn,
            path: paths.database,
        })
    }

    fn validate_metadata(conn: &Connection) -> Result<()> {
        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM tcui_credential_vault", [], |row| {
                row.get(0)
            })
            .map_err(|_| KeyStoreError::Read)?;
        if count != 1 {
            return Err(KeyStoreError::VaultAuthentication);
        }
        let (version, preference, salt, nonce, verifier) =
            conn.query_row(
                "SELECT version,preference,salt,verifier_nonce,verifier FROM tcui_credential_vault WHERE singleton=1",
                [], |row| Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, i64>(1)?,
                    row.get::<_, Option<Vec<u8>>>(2)?,
                    row.get::<_, Option<Vec<u8>>>(3)?,
                    row.get::<_, Option<Vec<u8>>>(4)?,
                )),
            ).map_err(|_| KeyStoreError::Read)?;
        if version != 1 {
            return Err(KeyStoreError::UnsupportedVersion {
                version: u32::try_from(version)
                    .map_err(|_| KeyStoreError::InvalidCredentialPayload)?,
            });
        }
        if !matches!(preference, 0 | 1) {
            return Err(KeyStoreError::VaultAuthentication);
        }
        match (salt, nonce, verifier) {
            (None, None, None) => {
                let encrypted: bool = conn
                    .query_row(
                        "SELECT EXISTS(SELECT 1 FROM tcui_credentials WHERE encrypted != 0)",
                        [],
                        |row| row.get(0),
                    )
                    .map_err(|_| KeyStoreError::Read)?;
                if encrypted {
                    return Err(KeyStoreError::VaultAuthentication);
                }
            }
            (Some(salt), Some(nonce), Some(verifier))
                if salt.len() == 16 && nonce.len() == 12 && verifier.len() == 32 => {}
            _ => return Err(KeyStoreError::VaultAuthentication),
        }
        Ok(())
    }

    pub fn initialized(&self) -> Result<bool> {
        self.conn
            .query_row(
                "SELECT salt IS NOT NULL FROM tcui_credential_vault WHERE singleton=1",
                [],
                |row| row.get(0),
            )
            .map_err(|_| KeyStoreError::Read)
    }

    pub fn unlocked(&self, keys: &UnlockKeys) -> bool {
        keys.contains_key(&self.path)
    }

    pub fn preference(&self) -> Result<CredentialStorage> {
        let encrypted: bool = self
            .conn
            .query_row(
                "SELECT preference FROM tcui_credential_vault WHERE singleton=1",
                [],
                |row| row.get(0),
            )
            .map_err(|_| KeyStoreError::Read)?;
        Ok(if encrypted {
            CredentialStorage::Encrypted
        } else {
            CredentialStorage::Plaintext
        })
    }

    pub fn provider_storage(&self, id: &str) -> Result<Option<CredentialStorage>> {
        let encrypted: Option<bool> = self.conn.query_row(
            "SELECT MAX(encrypted) FROM tcui_credentials WHERE id=?1 AND kind IN ('api_key','api_key_owned','oauth')",
            [id], |row| row.get(0),
        ).map_err(|_| KeyStoreError::Read)?;
        Ok(encrypted.map(|encrypted| {
            if encrypted {
                CredentialStorage::Encrypted
            } else {
                CredentialStorage::Plaintext
            }
        }))
    }

    pub fn selected_storage(
        &self,
        id: &str,
        selected: Option<CredentialStorage>,
    ) -> Result<CredentialStorage> {
        if let Some(storage) = self.provider_storage(id)? {
            if selected.is_some_and(|selected| selected != storage) {
                return Err(KeyStoreError::InvalidCredentialPayload);
            }
            return Ok(storage);
        }
        if let Some(storage) = selected {
            return Ok(storage);
        }
        match id
            .rsplit_once("::")
            .and_then(|(_, suffix)| suffix.split(':').next())
        {
            Some("encrypted") => Ok(CredentialStorage::Encrypted),
            Some("plaintext") => Ok(CredentialStorage::Plaintext),
            _ => self.preference(),
        }
    }

    pub fn set_default(&self, storage: CredentialStorage) -> Result<()> {
        self.conn
            .execute(
                "UPDATE tcui_credential_vault SET preference=?1 WHERE singleton=1",
                [storage == CredentialStorage::Encrypted],
            )
            .map_err(|_| KeyStoreError::Write)?;
        Ok(())
    }

    pub fn require_key<'a>(&self, keys: &'a UnlockKeys) -> Result<&'a [u8; 32]> {
        keys.get(&self.path)
            .map(|key| &**key)
            .ok_or(KeyStoreError::VaultLocked)
    }

    pub fn initialize(&self, password: &str, keys: &mut UnlockKeys) -> Result<()> {
        if self.initialized()? {
            return Err(KeyStoreError::VaultAlreadyInitialized);
        }
        let salt = rand::random::<[u8; 16]>();
        let key = derive_key(password, &salt)?;
        let (nonce, verifier) =
            encrypt(&key, b"tcui-credentials:v1:verifier", b"tcui-credentials")?;
        let changed = self.conn.execute("UPDATE tcui_credential_vault SET salt=?1,verifier_nonce=?2,verifier=?3 WHERE singleton=1 AND salt IS NULL", params![salt.as_slice(), nonce, verifier]).map_err(|_| KeyStoreError::Write)?;
        if changed != 1 {
            return Err(KeyStoreError::VaultAlreadyInitialized);
        }
        keys.insert(self.path.clone(), key);
        Ok(())
    }

    pub fn unlock(&self, password: &str, keys: &mut UnlockKeys) -> Result<()> {
        self.lock(keys);
        let (salt, nonce, verifier) = self
            .conn
            .query_row(
                "SELECT salt,verifier_nonce,verifier FROM tcui_credential_vault WHERE singleton=1",
                [],
                |row| {
                    Ok((
                        row.get::<_, Option<Vec<u8>>>(0)?,
                        row.get::<_, Option<Vec<u8>>>(1)?,
                        row.get::<_, Option<Vec<u8>>>(2)?,
                    ))
                },
            )
            .map_err(|_| KeyStoreError::Read)?;
        let salt = salt.ok_or(KeyStoreError::VaultNotInitialized)?;
        if salt.len() != 16 {
            return Err(KeyStoreError::VaultAuthentication);
        }
        let key = derive_key(password, &salt)?;
        let plaintext = decrypt(
            &key,
            b"tcui-credentials:v1:verifier",
            &nonce.ok_or(KeyStoreError::VaultAuthentication)?,
            &verifier.ok_or(KeyStoreError::VaultAuthentication)?,
        )?;
        if plaintext.as_slice() != b"tcui-credentials" {
            return Err(KeyStoreError::VaultAuthentication);
        }
        keys.insert(self.path.clone(), key);
        Ok(())
    }

    pub fn lock(&self, keys: &mut UnlockKeys) {
        keys.remove(&self.path);
    }

    pub fn transaction<T>(&self, action: impl FnOnce() -> Result<T>) -> Result<T> {
        self.conn
            .execute_batch("BEGIN IMMEDIATE")
            .map_err(|_| KeyStoreError::Write)?;
        match action() {
            Ok(value) => {
                if self.conn.execute_batch("COMMIT").is_err() {
                    let _ = self.conn.execute_batch("ROLLBACK");
                    return Err(KeyStoreError::Write);
                }
                Ok(value)
            }
            Err(error) => {
                let _ = self.conn.execute_batch("ROLLBACK");
                Err(error)
            }
        }
    }

    pub fn set_preference(&self, storage: CredentialStorage, keys: &UnlockKeys) -> Result<()> {
        if storage == CredentialStorage::Encrypted {
            self.require_key(keys)?;
        }
        if storage == CredentialStorage::Encrypted {
            // Refuse a conversion if another connection prevents removing the WAL.
            self.checkpoint()?;
            let mode: String = self
                .conn
                .query_row("PRAGMA journal_mode=DELETE", [], |row| row.get(0))
                .map_err(|_| KeyStoreError::Write)?;
            if !mode.eq_ignore_ascii_case("delete") {
                return Err(KeyStoreError::Write);
            }
        }
        self.transaction(|| {
            for record in self.records()? {
                if is_machine_kind(&record.kind) || record.storage == storage {
                    continue;
                }
                let payload = self.decode(&record, keys)?;
                self.write(
                    &record.kind,
                    &record.id,
                    &record.label,
                    &payload,
                    storage,
                    keys,
                )?;
            }
            self.conn
                .execute(
                    "UPDATE tcui_credential_vault SET preference=?1 WHERE singleton=1",
                    [storage == CredentialStorage::Encrypted],
                )
                .map_err(|_| KeyStoreError::Write)?;
            Ok(())
        })?;
        if storage == CredentialStorage::Encrypted {
            self.conn
                .execute_batch("VACUUM")
                .map_err(|_| KeyStoreError::Write)?;
            self.checkpoint()?;
        }
        Ok(())
    }

    fn checkpoint(&self) -> Result<()> {
        let busy: i64 = self
            .conn
            .query_row("PRAGMA wal_checkpoint(TRUNCATE)", [], |row| row.get(0))
            .map_err(|_| KeyStoreError::Write)?;
        if busy != 0 {
            return Err(KeyStoreError::Write);
        }
        Ok(())
    }

    pub fn records(&self) -> Result<Vec<Record>> {
        let mut statement = self.conn.prepare("SELECT kind,id,label,encrypted,nonce,payload FROM tcui_credentials ORDER BY kind,label,id").map_err(|_| KeyStoreError::Read)?;
        let rows = statement
            .query_map([], record_from_row)
            .map_err(|_| KeyStoreError::Read)?;
        rows.collect::<std::result::Result<Vec<_>, _>>()
            .map_err(|_| KeyStoreError::Read)
    }

    pub fn read(
        &self,
        kind: &str,
        id: &str,
        keys: &UnlockKeys,
    ) -> Result<Option<Zeroizing<Vec<u8>>>> {
        let record = self.conn.query_row("SELECT kind,id,label,encrypted,nonce,payload FROM tcui_credentials WHERE kind=?1 AND id=?2", params![kind,id], record_from_row).optional().map_err(|_| KeyStoreError::Read)?;
        record.map(|record| self.decode(&record, keys)).transpose()
    }

    fn decode(&self, record: &Record, keys: &UnlockKeys) -> Result<Zeroizing<Vec<u8>>> {
        if record.storage == CredentialStorage::Plaintext {
            if is_machine_kind(&record.kind) || !record.nonce.is_empty() {
                return Err(KeyStoreError::VaultAuthentication);
            }
            return Ok(Zeroizing::new(record.payload.to_vec()));
        }
        decrypt(
            self.require_key(keys)?,
            &metadata(&record.kind, &record.id, &record.label)?,
            &record.nonce,
            &record.payload,
        )
    }

    pub fn write(
        &self,
        kind: &str,
        id: &str,
        label: &str,
        payload: &[u8],
        storage: CredentialStorage,
        keys: &UnlockKeys,
    ) -> Result<()> {
        if id.trim().is_empty() || payload.is_empty() {
            return Err(KeyStoreError::InvalidCredentialPayload);
        }
        // A plaintext preference never permits overwriting a locked encrypted row.
        if storage == CredentialStorage::Plaintext {
            let encrypted: Option<bool> = self
                .conn
                .query_row(
                    "SELECT encrypted FROM tcui_credentials WHERE kind=?1 AND id=?2",
                    params![kind, id],
                    |row| row.get(0),
                )
                .optional()
                .map_err(|_| KeyStoreError::Read)?;
            if encrypted == Some(true) {
                self.require_key(keys)?;
            }
        }
        let (nonce, encrypted);
        let stored = if storage == CredentialStorage::Encrypted {
            (nonce, encrypted) = encrypt(
                self.require_key(keys)?,
                &metadata(kind, id, label)?,
                payload,
            )?;
            encrypted.as_slice()
        } else {
            if is_machine_kind(kind) {
                return Err(KeyStoreError::InvalidCredentialPayload);
            }
            nonce = Vec::new();
            payload
        };
        self.conn.execute("INSERT INTO tcui_credentials(kind,id,label,encrypted,nonce,payload) VALUES(?1,?2,?3,?4,?5,?6) ON CONFLICT(kind,id) DO UPDATE SET label=excluded.label,encrypted=excluded.encrypted,nonce=excluded.nonce,payload=excluded.payload", params![kind,id,label,storage == CredentialStorage::Encrypted,nonce,stored]).map_err(|_| KeyStoreError::Write)?;
        Ok(())
    }

    pub fn remove(&self, kind: &str, id: &str, keys: &UnlockKeys) -> Result<bool> {
        if self.read(kind, id, keys)?.is_none() {
            return Ok(false);
        }
        self.conn
            .execute(
                "DELETE FROM tcui_credentials WHERE kind=?1 AND id=?2",
                params![kind, id],
            )
            .map(|count| count != 0)
            .map_err(|_| KeyStoreError::Write)
    }
}

fn record_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<Record> {
    Ok(Record {
        kind: row.get(0)?,
        id: row.get(1)?,
        label: row.get(2)?,
        storage: if row.get::<_, bool>(3)? {
            CredentialStorage::Encrypted
        } else {
            CredentialStorage::Plaintext
        },
        nonce: row.get(4)?,
        payload: Zeroizing::new(row.get(5)?),
    })
}

pub(super) fn is_machine_kind(kind: &str) -> bool {
    matches!(kind, "machine_password" | "ssh_key" | "app_data_key")
}

fn derive_key(password: &str, salt: &[u8]) -> Result<Zeroizing<[u8; 32]>> {
    if password.is_empty() {
        return Err(KeyStoreError::EmptyPassword);
    }
    let params = Params::new(65536, 3, 1, Some(32)).map_err(|_| KeyStoreError::KeyAccess)?;
    let mut key = Zeroizing::new([0u8; 32]);
    Argon2::new(Algorithm::Argon2id, Version::V0x13, params)
        .hash_password_into(password.as_bytes(), salt, key.as_mut())
        .map_err(|_| KeyStoreError::KeyAccess)?;
    Ok(key)
}

fn metadata(kind: &str, id: &str, label: &str) -> Result<Vec<u8>> {
    serde_json::to_vec(&("tcui-credentials", 1, kind, id, label, "aes-256-gcm"))
        .map_err(|_| KeyStoreError::Serialize)
}

fn encrypt(key: &[u8; 32], aad: &[u8], plaintext: &[u8]) -> Result<(Vec<u8>, Vec<u8>)> {
    let cipher = Aes256Gcm::new_from_slice(key).map_err(|_| KeyStoreError::KeyAccess)?;
    let nonce = rand::random::<[u8; 12]>();
    let ciphertext = cipher
        .encrypt(
            (&nonce).into(),
            Payload {
                msg: plaintext,
                aad,
            },
        )
        .map_err(|_| KeyStoreError::CredentialEncrypt)?;
    Ok((nonce.to_vec(), ciphertext))
}

fn decrypt(
    key: &[u8; 32],
    aad: &[u8],
    nonce: &[u8],
    ciphertext: &[u8],
) -> Result<Zeroizing<Vec<u8>>> {
    let nonce: &[u8; 12] = nonce
        .try_into()
        .map_err(|_| KeyStoreError::VaultAuthentication)?;
    let cipher = Aes256Gcm::new_from_slice(key).map_err(|_| KeyStoreError::KeyAccess)?;
    cipher
        .decrypt(
            nonce.into(),
            Payload {
                msg: ciphertext,
                aad,
            },
        )
        .map(Zeroizing::new)
        .map_err(|_| KeyStoreError::VaultAuthentication)
}

#[cfg(test)]
mod tests {
    use super::*;
    use base64::{engine::general_purpose::STANDARD, Engine as _};

    #[test]
    fn decrypts_credential_written_before_crypto_upgrade() {
        // Generated with argon2 0.5.3 and aes-gcm 0.10.3, using the v1 vault parameters.
        let ciphertext = STANDARD
            .decode("T0sNNweFUydx3HfamRI7JyK5kxk+3m4gz8lYr+gOTUP1GrkW")
            .expect("decode fixture");
        let key = derive_key("dependency-compatibility-password", &[0x11; 16])
            .expect("derive existing vault key");
        let aad = metadata("machine_password", "fixture", "Fixture").expect("record metadata");
        let plaintext =
            decrypt(&key, &aad, &[7; 12], &ciphertext).expect("decrypt existing credential");
        assert_eq!(plaintext.as_slice(), b"local fixture secret");
    }
}
