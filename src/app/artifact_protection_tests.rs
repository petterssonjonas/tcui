use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::app::TuiApp;
use crate::config::vault_profiles::VaultProfile;
use crate::protection::{self, ProtectionPolicy};
use crate::ui::artifact_sidebar::ArtifactHandle;

struct Environment {
    root: PathBuf,
    previous: Vec<(&'static str, Option<std::ffi::OsString>)>,
}

impl Environment {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!(
            "tcui-memory-artifact-protection-{}",
            rand::random::<u64>()
        ));
        let previous = ["XDG_DATA_HOME", "XDG_CONFIG_HOME", "XDG_CACHE_HOME", "HOME"]
            .into_iter()
            .map(|name| (name, std::env::var_os(name)))
            .collect();
        for (name, directory) in [
            ("XDG_DATA_HOME", "data"),
            ("XDG_CONFIG_HOME", "config"),
            ("XDG_CACHE_HOME", "cache"),
            ("HOME", "home"),
        ] {
            std::fs::create_dir_all(root.join(directory)).expect("isolated directory");
            std::env::set_var(name, root.join(directory));
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

#[test]
fn protected_memory_export_requires_confirmation_and_stale_cache_cannot_view_export_or_delete() {
    let _guard = crate::test_support::env_lock()
        .lock()
        .expect("environment lock");
    let environment = Environment::new();
    let external = environment.root.join("external");
    std::fs::create_dir_all(&external).expect("external vault");
    std::fs::write(external.join("existing.md"), b"unrelated external note")
        .expect("external sentinel");
    let mut config = crate::config::AppConfig::load().expect("isolated config");
    let profile = VaultProfile::new("External", &external).expect("external profile");
    config.default_vault_id = Some(profile.id.clone());
    config.vaults.insert(profile.id.clone(), profile);
    config.vault_path = Some(external.display().to_string());
    config.save().expect("save external selection");
    protection::configure(
        ProtectionPolicy {
            memories: true,
            ..ProtectionPolicy::default()
        },
        "artifact-test-password",
    )
    .expect("protect memories");
    let store = crate::memory::MemoryStore::open_default().expect("memory store");
    store
        .write(
            Path::new("existing.md"),
            "# existing.md\n\nProtected memory body.",
            false,
        )
        .expect("protected memory");
    let physical = store.active_documents().expect("active document")[0]
        .0
        .clone();
    let ciphertext = std::fs::read(&physical).expect("memory ciphertext");
    let mut app = TuiApp::new(
        crate::storage::Storage::new().expect("chat storage"),
        Arc::new(tokio::sync::RwLock::new(config)),
        Arc::new(crate::llm::LlmClient::new()),
        None,
    );
    app.refresh_memory_artifacts();
    let artifact = app.ui.memory_artifacts[0].clone();
    let handle = artifact.handle.clone();
    assert!(matches!(handle, ArtifactHandle::Memory(_)));
    let before: Vec<_> = std::fs::read_dir(&external)
        .expect("before export")
        .map(|entry| entry.expect("entry").file_name())
        .collect();
    app.prepare_artifact_save(handle.clone());
    assert!(app.ui.export_dialog.is_some());
    assert!(app.ui.save_file_dialog.is_none());
    let after: Vec<_> = std::fs::read_dir(&external)
        .expect("no implicit export")
        .map(|entry| entry.expect("entry").file_name())
        .collect();
    assert_eq!(before, after);
    assert_eq!(
        std::fs::read(external.join("existing.md")).expect("sentinel retained"),
        b"unrelated external note"
    );

    app.ui
        .export_dialog
        .as_mut()
        .expect("export dialog")
        .directory_input = external.display().to_string();
    app.save_export_dialog().expect("confirmed export");
    let exported = std::fs::read_dir(&external)
        .expect("explicit export")
        .map(|entry| entry.expect("entry").path())
        .find(|path| path.file_name().is_some_and(|name| name != "existing.md"))
        .expect("collision-safe export filename");
    assert!(std::fs::read_to_string(exported)
        .expect("explicit plaintext export")
        .contains("Protected memory body."));
    assert_eq!(
        std::fs::read(external.join("existing.md")).expect("sentinel still retained"),
        b"unrelated external note"
    );

    app.ui.save_file_dialog = Some(crate::ui::modals::save_file::SaveFileDialog::new(
        &artifact,
        external.clone(),
        "Export",
    ));
    protection::lock().expect("invalidate session with cached artifacts still present");
    assert!(app.require_artifact_access(&handle).is_err());
    assert!(app.find_artifact(&handle).is_none());
    app.open_artifact(handle.clone());
    assert!(app.ui.artifact_viewer.is_none());
    assert!(app.save_generated_file().is_err());
    app.ui.save_file_dialog = None;
    app.prepare_artifact_save(handle.clone());
    assert!(app.ui.export_dialog.is_none());
    app.delete_artifact(handle.clone());
    assert_eq!(
        std::fs::read(&physical).expect("locked deletion preserved source"),
        ciphertext
    );
    protection::unlock("artifact-test-password").expect("unlock");
    assert_eq!(store.active_documents().expect("source retained").len(), 1);
    app.delete_artifact(handle);
    assert!(store
        .active_documents()
        .expect("memory moved to Trash")
        .is_empty());
    assert!(!physical.exists());
    assert!(crate::storage::paths::TcuiDataPaths::discover()
        .memories_trash_dir
        .read_dir()
        .expect("Trash exists")
        .next()
        .is_some());
}
