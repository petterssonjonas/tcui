use std::path::{Path, PathBuf};

use super::move_execute::MoveExecutor;
use super::move_journal::JournalStore;
use super::move_plan::{MovePair, MovePlan, VaultFileRef};
use super::{VaultFs, VaultRelativePath, VaultWalkLimits, VaultWriteService};
use crate::protection::{self, Domain, ProtectionPolicy};
use crate::storage::chat_files::ChatSavedFileStore;

struct Environment {
    root: PathBuf,
    previous: Vec<(&'static str, Option<std::ffi::OsString>)>,
}

impl Environment {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!(
            "tcui-internal-vault-protection-{}",
            rand::random::<u64>()
        ));
        let previous = ["XDG_DATA_HOME", "XDG_CONFIG_HOME", "XDG_CACHE_HOME", "HOME"]
            .into_iter()
            .map(|name| (name, std::env::var_os(name)))
            .collect();
        for (variable, directory) in [
            ("XDG_DATA_HOME", "data"),
            ("XDG_CONFIG_HOME", "config"),
            ("XDG_CACHE_HOME", "cache"),
            ("HOME", "home"),
        ] {
            std::fs::create_dir_all(root.join(directory)).expect("isolated directory");
            std::env::set_var(variable, root.join(directory));
        }
        Self { root, previous }
    }
}

impl Drop for Environment {
    fn drop(&mut self) {
        protection::lock().expect("clear isolated session");
        for (name, previous) in self.previous.drain(..) {
            match previous {
                Some(value) => std::env::set_var(name, value),
                None => std::env::remove_var(name),
            }
        }
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

fn relative(path: &str) -> VaultRelativePath {
    VaultRelativePath::parse(Path::new(path)).expect("relative path")
}

#[test]
fn native_saved_media_backups_and_moves_obey_protection() {
    let _guard = crate::test_support::env_lock()
        .lock()
        .expect("environment lock");
    let environment = Environment::new();
    let root = protection::internal_vault_root();
    std::fs::create_dir_all(&root).expect("native root");
    std::fs::write(root.join("existing.md"), b"existing native content").expect("native file");
    let external = environment.root.join("external-vault");
    std::fs::create_dir_all(&external).expect("external fixture");
    std::fs::write(external.join("ordinary.md"), b"external plaintext").expect("external sentinel");
    crate::config::AppConfig::default()
        .save()
        .expect("isolated config");
    protection::configure(
        ProtectionPolicy {
            vault: true,
            ..ProtectionPolicy::default()
        },
        "vault-test-password",
    )
    .expect("configure");
    assert_eq!(
        protection::read(Domain::Vault, &root.join("existing.md")).expect("native data"),
        b"existing native content"
    );
    assert_ne!(
        std::fs::read(root.join("existing.md")).expect("ciphertext"),
        b"existing native content"
    );
    assert_eq!(
        std::fs::read(external.join("ordinary.md")).expect("external sentinel"),
        b"external plaintext"
    );

    let saved = ChatSavedFileStore::discover();
    let payload = b"\x89PNG\0binary-secret-payload";
    let link = saved
        .create_for_chat(7, "image.png", payload)
        .expect("save binary");
    assert_eq!(saved.open_link(7, link.id).expect("read binary"), payload);
    assert_ne!(
        std::fs::read(saved.path_for_link(&link)).expect("ciphertext"),
        payload
    );
    let data = crate::storage::paths::TcuiDataPaths::discover();
    assert!(serde_json::from_slice::<serde_json::Value>(
        &std::fs::read(&data.chat_file_links_manifest).expect("manifest ciphertext")
    )
    .is_err());

    let trash_payload = b"private Trash media bytes";
    let trash_item = saved
        .create_item("trash.bin", trash_payload, Some(9))
        .expect("Trash fixture");
    saved
        .trash_item(trash_item.id)
        .expect("move item to protected Trash");
    assert!(!trash_item.path.exists());
    let manifest_bytes =
        protection::read(Domain::Vault, &data.chat_file_links_manifest).expect("decoded manifest");
    let manifest: serde_json::Value =
        serde_json::from_slice(&manifest_bytes).expect("manifest JSON");
    let archived = manifest["items"]
        .as_array()
        .expect("item index")
        .iter()
        .find(|item| item["id"].as_u64() == Some(trash_item.id.get()))
        .expect("stable trashed ID");
    assert_eq!(archived["state"]["status"], "trashed");
    assert_eq!(archived["archived_links"][0]["conversation_id"], 9);
    let trash_path = root.join(
        archived["state"]["trash_path"]
            .as_str()
            .expect("Trash path"),
    );
    assert_ne!(
        std::fs::read(&trash_path).expect("Trash ciphertext"),
        trash_payload
    );
    assert_eq!(
        protection::read(Domain::Vault, &trash_path).expect("decoded Trash body"),
        trash_payload
    );
    assert!(serde_json::from_slice::<serde_json::Value>(
        &std::fs::read(&data.chat_file_links_manifest).expect("protected Trash manifest")
    )
    .is_err());

    let service = VaultWriteService::new().expect("service");
    let first = service
        .write(&relative("existing.md"), b"replacement")
        .expect("protected write");
    assert!(service
        .validate_editor_target(&relative("existing.md"))
        .is_err());
    service
        .restore(&relative("existing.md"), &first.backup_id)
        .expect("restore decoded backup");
    assert_eq!(
        VaultFs::open(&root)
            .expect("Vault")
            .read_text(&relative("existing.md"))
            .expect("restored"),
        "existing native content"
    );
    let backup = data
        .root
        .join("vault-backups")
        .join(first.backup_id.as_str());
    assert_ne!(
        std::fs::read(backup.join("original.bin")).expect("backup ciphertext"),
        b"existing native content"
    );
    assert!(serde_json::from_slice::<serde_json::Value>(
        &std::fs::read(backup.join("envelope.json")).expect("metadata ciphertext")
    )
    .is_err());

    let transfer = MovePlan::build(
        vec![MovePair::new(
            VaultFileRef::new(relative("existing.md")),
            VaultFileRef::new(relative("archive/moved.md")),
            false,
        )],
        &data.root,
        |_| false,
    )
    .expect("native plan");
    let outcome = MoveExecutor::at_data_root(&data.root)
        .execute(transfer)
        .expect("protected native move");
    assert!(!root.join("existing.md").exists());
    assert_eq!(
        protection::read(Domain::Vault, &root.join("archive/moved.md"))
            .expect("moved native content"),
        b"existing native content"
    );
    assert_ne!(
        std::fs::read(root.join("archive/moved.md")).expect("moved ciphertext"),
        b"existing native content"
    );
    let journal = data
        .root
        .join("vault-native-ops")
        .join(outcome.operation_id.as_str())
        .join("journal.json");
    assert!(serde_json::from_slice::<serde_json::Value>(
        &std::fs::read(journal).expect("journal ciphertext")
    )
    .is_err());

    let open_handle = VaultFs::open(&root).expect("open handle");
    protection::lock().expect("lock");
    assert!(open_handle
        .read_text(&relative("archive/moved.md"))
        .is_err());
    assert!(open_handle.list(None, VaultWalkLimits::default()).is_err());
    assert!(saved.open_link(7, link.id).is_err());
    assert!(saved.list_saved().is_err());
    assert!(JournalStore::at_data_root(&data.root)
        .load(&outcome.operation_id)
        .is_err());
    assert!(service
        .restore(&relative("existing.md"), &first.backup_id)
        .is_err());
    assert!(protection::unlock("wrong-password").is_err());
    assert!(saved.open_link(7, link.id).is_err());
    assert_eq!(
        std::fs::read(external.join("ordinary.md")).expect("external untouched"),
        b"external plaintext"
    );
    protection::unlock("vault-test-password").expect("unlock");
    assert_eq!(
        saved.open_link(7, link.id).expect("binary round trip"),
        payload
    );
}

#[path = "cutover_tests.rs"]
mod cutover;
