#![allow(dead_code)]

use aes_gcm::{
    Aes256Gcm,
    aead::{Aead, KeyInit, Payload},
};
use base64::{Engine as _, engine::general_purpose::STANDARD};
use serde::{Serialize, de::DeserializeOwned};
use std::fs::OpenOptions;
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use crate::storage::paths::{
    TcuiDataPaths, directory_has_entries, directory_has_non_trash_entries, ensure_directory,
    set_unix_mode,
};

const ENVELOPE_PREFIX: &str = "enc:v1:";

#[derive(Debug, Clone)]
pub(crate) struct SharedKey {
    bytes: [u8; 32],
}

#[derive(Debug, Clone)]
pub(crate) struct SharedKeyLoad {
    pub(crate) key: SharedKey,
    pub(crate) created_default_key: bool,
}

#[derive(Debug, thiserror::Error)]
pub(crate) enum StorageCryptoError {
    #[error("failed to access local storage path: {0}")]
    Io(#[from] io::Error),
    #[error("invalid local storage key encoding: {0}")]
    Base64(#[from] base64::DecodeError),
    #[error("invalid local storage key length")]
    InvalidKeyLength,
    #[error("missing local storage nonce")]
    MissingNonce,
    #[error("missing local storage ciphertext")]
    MissingCiphertext,
    #[error("invalid local storage envelope")]
    InvalidEnvelope,
    #[error("failed to encrypt local secret")]
    Encrypt,
    #[error("failed to decrypt local secret")]
    Decrypt,
    #[error("encrypted document kind mismatch")]
    WrongDocumentKind,
    #[error("invalid encrypted document json: {0}")]
    Json(#[from] serde_json::Error),
    #[error("invalid utf-8 in encrypted payload: {0}")]
    Utf8(#[from] std::string::FromUtf8Error),
    #[error(
        "missing local storage key at {path}; restore it before starting tcui because {contexts}"
    )]
    MissingDefaultKey { path: PathBuf, contexts: String },
}

impl SharedKey {
    pub(crate) fn load_or_create_default(
        paths: &TcuiDataPaths,
    ) -> Result<SharedKeyLoad, StorageCryptoError> {
        paths.ensure_layout()?;
        if paths.chat_key.exists() {
            let key = Self::read_from_path(&paths.chat_key)?;
            set_unix_mode(&paths.chat_key, 0o400)?;
            return Ok(SharedKeyLoad {
                key,
                created_default_key: false,
            });
        }

        if let Some(contexts) = encrypted_default_data_context(paths)? {
            return Err(StorageCryptoError::MissingDefaultKey {
                path: paths.chat_key.clone(),
                contexts,
            });
        }

        match create_key_file(&paths.chat_key) {
            Ok(key) => Ok(SharedKeyLoad {
                key,
                created_default_key: true,
            }),
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
                let key = Self::read_from_path(&paths.chat_key)?;
                set_unix_mode(&paths.chat_key, 0o400)?;
                Ok(SharedKeyLoad {
                    key,
                    created_default_key: false,
                })
            }
            Err(error) => Err(StorageCryptoError::Io(error)),
        }
    }

    #[allow(dead_code)]
    pub(crate) fn load_from_path(path: &Path) -> Result<Self, StorageCryptoError> {
        Self::read_from_path(path)
    }

    fn read_from_path(path: &Path) -> Result<Self, StorageCryptoError> {
        let encoded = std::fs::read_to_string(path)?;
        let decoded = STANDARD.decode(encoded.trim())?;
        let bytes: [u8; 32] = decoded
            .try_into()
            .map_err(|_| StorageCryptoError::InvalidKeyLength)?;
        Ok(Self { bytes })
    }

    fn cipher(&self) -> Result<Aes256Gcm, StorageCryptoError> {
        Aes256Gcm::new_from_slice(&self.bytes).map_err(|_| StorageCryptoError::InvalidKeyLength)
    }
}

pub(crate) fn encrypt_shared_text_with_key(
    key: &SharedKey,
    plaintext: &str,
) -> Result<String, StorageCryptoError> {
    if plaintext.is_empty() {
        return Ok(String::new());
    }

    let ciphertext = encrypt_bytes(key, plaintext.as_bytes(), b"")?;
    Ok(format_envelope(&ciphertext.0, &ciphertext.1))
}

pub(crate) fn decrypt_shared_text_with_key(
    key: &SharedKey,
    stored: &str,
) -> Result<String, StorageCryptoError> {
    let Some((nonce, ciphertext)) = parse_optional_envelope(stored)? else {
        return Ok(stored.to_string());
    };
    let plaintext = decrypt_bytes(key, &nonce, &ciphertext, b"")?;
    Ok(String::from_utf8(plaintext)?)
}

pub(crate) fn encrypt_serialized<T: Serialize>(
    key: &SharedKey,
    kind: &str,
    value: &T,
) -> Result<String, StorageCryptoError> {
    let plaintext = serde_json::to_vec(value)?;
    let (nonce, ciphertext) = encrypt_bytes(key, &plaintext, kind.as_bytes())?;
    Ok(format_envelope(&nonce, &ciphertext))
}

pub(crate) fn decrypt_serialized<T: DeserializeOwned>(
    key: &SharedKey,
    kind: &str,
    stored: &str,
) -> Result<T, StorageCryptoError> {
    let (nonce, ciphertext) = parse_required_envelope(stored)?;
    let plaintext = decrypt_bytes(key, &nonce, &ciphertext, kind.as_bytes())?;
    Ok(serde_json::from_slice(&plaintext)?)
}

pub(crate) fn write_encrypted_document<T: Serialize>(
    path: &Path,
    key: &SharedKey,
    kind: &str,
    value: &T,
) -> Result<(), StorageCryptoError> {
    let payload = encrypt_serialized(key, kind, value)?;
    write_atomic_text(path, &payload)
}

pub(crate) fn read_encrypted_document<T: DeserializeOwned>(
    path: &Path,
    key: &SharedKey,
    kind: &str,
) -> Result<T, StorageCryptoError> {
    let stored = std::fs::read_to_string(path)?;
    decrypt_serialized(key, kind, &stored)
}

pub(crate) fn write_atomic_text(path: &Path, contents: &str) -> Result<(), StorageCryptoError> {
    let parent = path
        .parent()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "path has no parent"))?;
    ensure_directory(parent)?;
    let temp_name = format!(
        ".{}.{}.tmp",
        path.file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("tcui"),
        rand::random::<u64>()
    );
    let temp_path = parent.join(temp_name);

    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temp_path)?;
    file.write_all(contents.as_bytes())?;
    file.sync_all()?;
    drop(file);

    std::fs::rename(&temp_path, path)?;
    sync_parent(parent)?;
    Ok(())
}

fn create_key_file(path: &Path) -> io::Result<SharedKey> {
    let key = SharedKey {
        bytes: rand::random::<[u8; 32]>(),
    };
    let encoded = STANDARD.encode(key.bytes);
    let mut file = OpenOptions::new().write(true).create_new(true).open(path)?;
    file.write_all(encoded.as_bytes())?;
    file.sync_all()?;
    drop(file);
    set_unix_mode(path, 0o400)?;
    sync_parent(path.parent().unwrap_or(Path::new(".")))?;
    Ok(key)
}

fn encrypt_bytes(
    key: &SharedKey,
    plaintext: &[u8],
    aad: &[u8],
) -> Result<(Vec<u8>, Vec<u8>), StorageCryptoError> {
    let nonce = rand::random::<[u8; 12]>().to_vec();
    let ciphertext = key
        .cipher()?
        .encrypt(
            nonce.as_slice().into(),
            Payload {
                msg: plaintext,
                aad,
            },
        )
        .map_err(|_| StorageCryptoError::Encrypt)?;
    Ok((nonce, ciphertext))
}

fn decrypt_bytes(
    key: &SharedKey,
    nonce: &[u8],
    ciphertext: &[u8],
    aad: &[u8],
) -> Result<Vec<u8>, StorageCryptoError> {
    key.cipher()?
        .decrypt(
            nonce.into(),
            Payload {
                msg: ciphertext,
                aad,
            },
        )
        .map_err(|_| {
            if aad.is_empty() {
                StorageCryptoError::Decrypt
            } else {
                StorageCryptoError::WrongDocumentKind
            }
        })
}

type Envelope = (Vec<u8>, Vec<u8>);

fn parse_optional_envelope(stored: &str) -> Result<Option<Envelope>, StorageCryptoError> {
    let Some(encoded) = stored.strip_prefix(ENVELOPE_PREFIX) else {
        return Ok(None);
    };
    Ok(Some(parse_envelope(encoded)?))
}

fn parse_required_envelope(stored: &str) -> Result<Envelope, StorageCryptoError> {
    let encoded = stored
        .strip_prefix(ENVELOPE_PREFIX)
        .ok_or(StorageCryptoError::InvalidEnvelope)?;
    parse_envelope(encoded)
}

fn parse_envelope(encoded: &str) -> Result<Envelope, StorageCryptoError> {
    let mut parts = encoded.splitn(2, ':');
    let nonce = parts.next().ok_or(StorageCryptoError::MissingNonce)?;
    let ciphertext = parts.next().ok_or(StorageCryptoError::MissingCiphertext)?;
    let nonce = STANDARD.decode(nonce)?;
    let ciphertext = STANDARD.decode(ciphertext)?;
    Ok((nonce, ciphertext))
}

fn format_envelope(nonce: &[u8], ciphertext: &[u8]) -> String {
    format!(
        "{ENVELOPE_PREFIX}{}:{}",
        STANDARD.encode(nonce),
        STANDARD.encode(ciphertext)
    )
}

fn encrypted_default_data_context(
    paths: &TcuiDataPaths,
) -> Result<Option<String>, StorageCryptoError> {
    let mut contexts = Vec::new();
    for (label, path) in [
        ("active chats", &paths.chats_dir),
        ("active memories", &paths.memories_dir),
    ] {
        if directory_has_non_trash_entries(path)? {
            contexts.push(format!("{label} exist at {}", path.display()));
        }
    }
    for (label, path) in [
        ("trashed chats", &paths.chats_trash_dir),
        ("trashed memories", &paths.memories_trash_dir),
    ] {
        if directory_has_entries(path)? {
            contexts.push(format!("{label} exist at {}", path.display()));
        }
    }
    if keys_file_contains_encrypted_data(&paths.keys_file)? {
        contexts.push(format!(
            "saved provider keys exist at {}",
            paths.keys_file.display()
        ));
    }

    if contexts.is_empty() {
        Ok(None)
    } else {
        Ok(Some(contexts.join("; ")))
    }
}

fn keys_file_contains_encrypted_data(path: &Path) -> Result<bool, StorageCryptoError> {
    if !path.exists() {
        return Ok(false);
    }
    let content = std::fs::read_to_string(path)?;
    Ok(content.contains(ENVELOPE_PREFIX))
}

fn sync_parent(path: &Path) -> io::Result<()> {
    #[cfg(unix)]
    {
        let directory = OpenOptions::new().read(true).open(path)?;
        directory.sync_all()
    }

    #[cfg(not(unix))]
    let _ = path;

    #[cfg(not(unix))]
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::paths::TcuiDataPaths;
    use serde::{Deserialize, Serialize};

    #[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
    struct LocalCipherDocument {
        title: String,
        body: String,
    }

    fn env_lock() -> &'static std::sync::Mutex<()> {
        crate::test_support::env_lock()
    }

    fn unique_temp_dir(label: &str) -> PathBuf {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("system clock before epoch")
            .as_nanos();
        std::env::temp_dir().join(format!("tcui-{label}-{}-{nanos}", std::process::id()))
    }

    fn set_xdg_root(root: &Path) {
        std::env::set_var("XDG_DATA_HOME", root);
    }

    fn clear_xdg_root() {
        std::env::remove_var("XDG_DATA_HOME");
    }

    #[test]
    fn local_cipher_creates_default_key_and_tcui_directories() {
        let _guard = env_lock()
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        let root = unique_temp_dir("local-cipher-create");
        set_xdg_root(&root);

        let paths = TcuiDataPaths::discover();
        let loaded = SharedKey::load_or_create_default(&paths).expect("create shared key");

        assert!(loaded.created_default_key);
        assert!(paths.chat_key.exists());
        assert!(paths.chats_dir.exists());
        assert!(paths.chats_trash_dir.exists());
        assert!(paths.memories_dir.exists());
        assert!(paths.memories_trash_dir.exists());
        let decoded = STANDARD
            .decode(
                std::fs::read_to_string(&paths.chat_key)
                    .expect("read key")
                    .trim(),
            )
            .expect("decode key");
        assert_eq!(decoded.len(), 32);

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                std::fs::metadata(&paths.chat_key)
                    .expect("key metadata")
                    .permissions()
                    .mode()
                    & 0o777,
                0o400
            );
            for directory in [
                &paths.root,
                &paths.chats_dir,
                &paths.chats_trash_dir,
                &paths.memories_dir,
                &paths.memories_trash_dir,
            ] {
                assert_eq!(
                    std::fs::metadata(directory)
                        .expect("dir metadata")
                        .permissions()
                        .mode()
                        & 0o777,
                    0o700
                );
            }
        }

        std::fs::remove_dir_all(&root).expect("cleanup temp dir");
        clear_xdg_root();
    }

    #[test]
    fn local_cipher_refuses_to_replace_missing_default_key_when_provider_data_exists() {
        let _guard = env_lock()
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        let root = unique_temp_dir("local-cipher-missing-key");
        set_xdg_root(&root);

        let paths = TcuiDataPaths::discover();
        paths.ensure_layout().expect("create layout");
        std::fs::write(
            &paths.keys_file,
            "keys = { OpenAI = \"enc:v1:ZmFrZQ==:ZmFrZQ==\" }\n",
        )
        .expect("write provider keys");

        let error = SharedKey::load_or_create_default(&paths).expect_err("missing key should fail");
        match error {
            StorageCryptoError::MissingDefaultKey { path, contexts } => {
                assert_eq!(path, paths.chat_key);
                assert!(contexts.contains("saved provider keys exist"));
            }
            other => panic!("unexpected error: {other}"),
        }
        assert!(
            !paths.chat_key.exists(),
            "replacement key should not be created"
        );

        std::fs::remove_dir_all(&root).expect("cleanup temp dir");
        clear_xdg_root();
    }

    #[test]
    fn local_cipher_encrypts_documents_without_plaintext() {
        let _guard = env_lock()
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        let root = unique_temp_dir("local-cipher-doc");
        set_xdg_root(&root);

        let paths = TcuiDataPaths::discover();
        let loaded = SharedKey::load_or_create_default(&paths).expect("create shared key");
        let path = paths.chats_dir.join("document.tcui-chat");
        let document = LocalCipherDocument {
            title: "Private title".to_string(),
            body: "Do not leak this sentence.".to_string(),
        };

        write_encrypted_document(&path, &loaded.key, "chat", &document).expect("write document");
        let raw = std::fs::read_to_string(&path).expect("read encrypted document");
        assert!(raw.starts_with(ENVELOPE_PREFIX));
        assert!(!raw.contains("Private title"));
        assert!(!raw.contains("Do not leak this sentence."));

        let loaded_document: LocalCipherDocument =
            read_encrypted_document(&path, &loaded.key, "chat").expect("read document");
        assert_eq!(loaded_document, document);

        std::fs::remove_dir_all(&root).expect("cleanup temp dir");
        clear_xdg_root();
    }

    #[test]
    fn local_cipher_rejects_wrong_key_truncated_data_and_wrong_document_kind() {
        let _guard = env_lock()
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        let root = unique_temp_dir("local-cipher-errors");
        set_xdg_root(&root);

        let paths = TcuiDataPaths::discover();
        let loaded = SharedKey::load_or_create_default(&paths).expect("create shared key");
        let wrong_key = SharedKey {
            bytes: rand::random::<[u8; 32]>(),
        };
        let document = LocalCipherDocument {
            title: "Encrypted".to_string(),
            body: "Body".to_string(),
        };
        let stored = encrypt_serialized(&loaded.key, "chat", &document).expect("encrypt document");

        let wrong_key_error =
            decrypt_serialized::<LocalCipherDocument>(&wrong_key, "chat", &stored)
                .expect_err("wrong key should fail");
        assert!(matches!(
            wrong_key_error,
            StorageCryptoError::WrongDocumentKind
        ));

        let truncated_error =
            decrypt_serialized::<LocalCipherDocument>(&loaded.key, "chat", "enc:v1:not-base64")
                .expect_err("truncated envelope should fail");
        assert!(
            matches!(
                truncated_error,
                StorageCryptoError::MissingCiphertext | StorageCryptoError::Base64(_)
            ),
            "unexpected truncated error: {truncated_error}"
        );

        let wrong_kind_error =
            decrypt_serialized::<LocalCipherDocument>(&loaded.key, "memory", &stored)
                .expect_err("wrong document kind should fail");
        assert!(matches!(
            wrong_kind_error,
            StorageCryptoError::WrongDocumentKind
        ));

        let (nonce, ciphertext) =
            encrypt_bytes(&loaded.key, b"not-json", b"chat").expect("encrypt invalid json");
        let invalid_json = format_envelope(&nonce, &ciphertext);
        let invalid_json_error =
            decrypt_serialized::<LocalCipherDocument>(&loaded.key, "chat", &invalid_json)
                .expect_err("invalid json should fail");
        assert!(matches!(invalid_json_error, StorageCryptoError::Json(_)));

        std::fs::remove_dir_all(&root).expect("cleanup temp dir");
        clear_xdg_root();
    }
}
