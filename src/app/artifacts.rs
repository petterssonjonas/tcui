use std::collections::HashSet;

use super::TuiApp;

#[cfg(all(test, feature = "memory"))]
#[path = "artifact_protection_tests.rs"]
mod protection_tests;

impl TuiApp {
    pub(crate) fn require_artifact_access(
        &self,
        handle: &crate::ui::artifact_sidebar::ArtifactHandle,
    ) -> color_eyre::Result<()> {
        use crate::ui::artifact_sidebar::ArtifactHandle;
        crate::protection::require_app()?;
        match handle {
            ArtifactHandle::Memory(_) => {
                crate::protection::require(crate::protection::Domain::Memories)
            }
            ArtifactHandle::Vault(_) | ArtifactHandle::ChatFile { .. } => {
                crate::protection::require(crate::protection::Domain::Vault)
            }
            ArtifactHandle::Temporary(_) | ArtifactHandle::Media(_) => Ok(()),
        }
    }

    pub(crate) fn show_resource_error(
        &mut self,
        domain: crate::protection::Domain,
        error: color_eyre::Report,
    ) {
        let adoption = error.chain().any(|cause| {
            cause.is::<crate::protection::NativeStoreAdoptionNeeded>()
                || cause.is::<crate::protection::PortableUnlockNeeded>()
        });
        self.ui.show_toast(error.to_string());
        if adoption {
            self.request_security_restart(crate::protection::ui::SecurityRestart::Transfer(domain));
        } else if crate::protection::is_protected(domain).unwrap_or(false)
            && !crate::protection::is_unlocked().unwrap_or(false)
        {
            self.request_security_restart(crate::protection::ui::SecurityRestart::Unlock);
        }
    }

    pub(crate) fn refresh_vault_artifacts(&mut self) {
        self.ui.vault_artifacts.clear();
        self.ui.vault_available = false;
        match self.chat_files.list_items() {
            Ok(records) => {
                self.ui.vault_artifacts = records
                    .into_iter()
                    .map(crate::ui::artifact_sidebar::ArtifactEntry::vault_file)
                    .collect();
                self.ui
                    .vault_artifacts
                    .sort_by(|left, right| left.relative_path.cmp(&right.relative_path));
                self.ui.vault_available = true;
                self.ui.vault_notice = None;
            }
            Err(error) => self.ui.vault_notice = Some(error.to_string()),
        }
    }

    pub(crate) fn refresh_memory_artifacts(&mut self) -> color_eyre::Result<()> {
        self.ui.memory_artifacts.clear();
        #[cfg(feature = "memory")]
        {
            let entries = crate::memory::MemoryStore::open_default()?.list_entries()?;
            self.ui.memory_artifacts = entries
                .into_iter()
                .map(|entry| {
                    crate::ui::artifact_sidebar::ArtifactEntry::memory_file(entry.id, entry.title)
                })
                .collect();
            self.ui.memory_notice = None;
            self.ui.memory_manager.reconcile(&self.ui.memory_artifacts);
        }
        Ok(())
    }

    pub(crate) fn refresh_artifact_sidebar_catalogs(&mut self) {
        self.ui.artifact_sidebar_state.clear_hit_areas();
        self.refresh_vault_artifacts();
        if let Err(error) = self.refresh_memory_artifacts() {
            self.ui.memory_notice = Some(error.to_string());
        }
    }

    pub(crate) fn artifact_bytes(
        &self,
        handle: &crate::ui::artifact_sidebar::ArtifactHandle,
    ) -> color_eyre::Result<Vec<u8>> {
        use crate::ui::artifact_sidebar::ArtifactHandle;
        self.require_artifact_access(handle)?;
        match handle {
            ArtifactHandle::Vault(id) => Ok(self.chat_files.read_item(*id)?),
            ArtifactHandle::ChatFile {
                conversation_id,
                id,
            } => Ok(self.chat_files.open_link(*conversation_id, *id)?),
            ArtifactHandle::Memory(id) => {
                #[cfg(feature = "memory")]
                {
                    Ok(crate::memory::MemoryStore::open_default()?
                        .read_entry(*id)?
                        .into_bytes())
                }
                #[cfg(not(feature = "memory"))]
                {
                    let _ = id;
                    color_eyre::eyre::bail!("Memories are not enabled in this build")
                }
            }
            ArtifactHandle::Temporary(_) => self
                .find_artifact(handle)
                .and_then(|artifact| artifact.content)
                .map(String::into_bytes)
                .ok_or_else(|| color_eyre::eyre::eyre!("Chat artifact is unavailable")),
            ArtifactHandle::Media(_) => {
                let path = self
                    .find_artifact(handle)
                    .and_then(|artifact| artifact.path)
                    .ok_or_else(|| color_eyre::eyre::eyre!("Chat artifact is unavailable"))?;
                Ok(crate::vault::read_artifact_path(&path)?)
            }
        }
    }

    pub(crate) fn artifact_domain(
        handle: &crate::ui::artifact_sidebar::ArtifactHandle,
    ) -> crate::protection::Domain {
        match handle {
            crate::ui::artifact_sidebar::ArtifactHandle::Memory(_) => {
                crate::protection::Domain::Memories
            }
            _ => crate::protection::Domain::Vault,
        }
    }

    pub(crate) fn prepare_artifact_save(
        &mut self,
        handle: crate::ui::artifact_sidebar::ArtifactHandle,
    ) {
        if let crate::ui::artifact_sidebar::ArtifactHandle::Memory(id) = handle {
            self.open_memory_export_dialog(id);
            return;
        }
        let Some(artifact) = self.find_artifact(&handle) else {
            return;
        };
        if matches!(
            artifact.origin,
            crate::ui::artifact_sidebar::ArtifactOrigin::Vault
        ) {
            return;
        }
        if artifact.origin == crate::ui::artifact_sidebar::ArtifactOrigin::Temporary {
            match self.save_temporary_artifact(&artifact) {
                Ok(()) => self.ui.show_toast("Saved a new Vault snapshot.".into()),
                Err(error) => self.show_resource_error(crate::protection::Domain::Vault, error),
            }
            return;
        }
        let result = self
            .artifact_bytes(&handle)
            .and_then(|bytes| Ok(self.chat_files.create_item(&artifact.name, &bytes, None)?));
        match result {
            Ok(record) => {
                if matches!(
                    handle,
                    crate::ui::artifact_sidebar::ArtifactHandle::Temporary(_)
                        | crate::ui::artifact_sidebar::ArtifactHandle::Media(_)
                ) {
                    self.remove_temporary_artifact(&handle);
                }
                self.refresh_artifact_sidebar_catalogs();
                self.ui.show_toast(format!(
                    "Saved a Vault snapshot as {} (#{}).",
                    record.filename, record.id
                ));
            }
            Err(error) => self.show_resource_error(crate::protection::Domain::Vault, error),
        }
    }

    pub(crate) fn save_temporary_artifact(
        &mut self,
        artifact: &crate::ui::artifact_sidebar::ArtifactEntry,
    ) -> color_eyre::Result<()> {
        self.require_artifact_access(&artifact.handle)?;
        if artifact.origin != crate::ui::artifact_sidebar::ArtifactOrigin::Temporary {
            color_eyre::eyre::bail!("Only temporary chat artifacts can be promoted directly");
        }
        let bytes = artifact
            .content
            .as_deref()
            .ok_or_else(|| color_eyre::eyre::eyre!("Chat artifact content is unavailable"))?;
        self.chat_files
            .create_item(&artifact.name, bytes.as_bytes(), None)?;
        self.remove_temporary_artifact(&artifact.handle);
        self.refresh_artifact_sidebar_catalogs();
        Ok(())
    }

    pub(crate) fn promote_temporary_artifact_if_vault_path(
        &mut self,
        handle: &crate::ui::artifact_sidebar::ArtifactHandle,
        saved_path: &std::path::Path,
    ) {
        if crate::vault::is_internal_root(saved_path) {
            self.remove_temporary_artifact(handle);
            self.refresh_artifact_sidebar_catalogs();
        }
    }

    pub(crate) fn open_artifact(&mut self, handle: crate::ui::artifact_sidebar::ArtifactHandle) {
        if let Err(error) = self.require_artifact_access(&handle) {
            self.show_resource_error(Self::artifact_domain(&handle), error);
            return;
        }
        let Some(mut artifact) = self.find_artifact(&handle) else {
            return;
        };
        match self.artifact_bytes(&handle) {
            Ok(bytes) => {
                if matches!(
                    artifact.kind,
                    crate::ui::artifact_sidebar::ArtifactKind::Markdown
                        | crate::ui::artifact_sidebar::ArtifactKind::Text
                ) {
                    artifact.content = String::from_utf8(bytes).ok();
                }
                self.ui.artifact_viewer =
                    Some(crate::ui::modals::artifact_viewer::ArtifactViewerState::new(artifact));
            }
            Err(error) => self.show_resource_error(Self::artifact_domain(&handle), error),
        }
    }

    pub(crate) fn edit_artifact(&mut self, handle: crate::ui::artifact_sidebar::ArtifactHandle) {
        if let Err(error) = self.require_artifact_access(&handle) {
            self.show_resource_error(Self::artifact_domain(&handle), error);
            return;
        }
        if !matches!(
            handle,
            crate::ui::artifact_sidebar::ArtifactHandle::Vault(_)
                | crate::ui::artifact_sidebar::ArtifactHandle::Memory(_)
        ) {
            self.ui
                .show_toast("Save this chat artifact to the Vault before editing it.".into());
            return;
        }
        let Some(artifact) = self.find_artifact(&handle) else {
            return;
        };
        let bytes = match self.artifact_bytes(&handle) {
            Ok(bytes) => bytes,
            Err(error) => {
                self.show_resource_error(Self::artifact_domain(&handle), error);
                return;
            }
        };
        let result =
            crate::ui::modals::resource_editor::ResourceEditor::new(handle, artifact.name, bytes);
        match result {
            Ok(editor) => {
                self.ui.artifact_viewer = None;
                self.ui.resource_editor = Some(editor);
            }
            Err(error) => self.ui.show_toast(error),
        }
    }

    pub(crate) fn copy_artifact(&mut self, handle: crate::ui::artifact_sidebar::ArtifactHandle) {
        use base64::Engine;
        let result = self.artifact_bytes(&handle).and_then(|bytes| {
            let text = std::str::from_utf8(&bytes).ok().filter(|text| {
                !text
                    .chars()
                    .any(|ch| ch.is_control() && !matches!(ch, '\n' | '\r' | '\t'))
            });
            if let Some(text) = text {
                crate::clipboard::copy(text)?;
                Ok("Clipboard request sent as plaintext.")
            } else {
                let encoded = base64::engine::general_purpose::STANDARD.encode(&bytes);
                crate::clipboard::copy(&encoded)?;
                Ok("Binary content sent to the clipboard as base64.")
            }
        });
        match result {
            Ok(notice) => self.ui.show_toast(notice.into()),
            Err(error) => self.show_resource_error(Self::artifact_domain(&handle), error),
        }
    }

    pub(crate) fn save_resource_editor(&mut self) {
        let Some(editor) = self.ui.resource_editor.as_ref() else {
            return;
        };
        let handle = editor.handle.clone();
        let mut saved_digest = None;
        let result = (|| -> color_eyre::Result<Option<String>> {
            self.require_artifact_access(&handle)?;
            let bytes = editor.bytes().map_err(color_eyre::eyre::Error::msg)?;
            let digest = blake3::hash(&bytes);
            let warning = match handle {
                crate::ui::artifact_sidebar::ArtifactHandle::Vault(id) => {
                    self.chat_files
                        .replace_item_if_unchanged(id, &bytes, editor.source_digest)?;
                    None
                }
                crate::ui::artifact_sidebar::ArtifactHandle::Memory(id) => {
                    #[cfg(feature = "memory")]
                    {
                        let markdown = std::str::from_utf8(&bytes)?;
                        match crate::memory::MemoryStore::open_default()?
                            .replace_entry_if_unchanged(id, markdown, editor.source_digest)
                        {
                            Ok(_) => None,
                            Err(error @ crate::memory::MemoryError::SavedButNotIndexed { .. }) => {
                                Some(error.to_string())
                            }
                            Err(error) => return Err(error.into()),
                        }
                    }
                    #[cfg(not(feature = "memory"))]
                    {
                        let _ = id;
                        color_eyre::eyre::bail!("Memories are not enabled in this build");
                    }
                }
                _ => color_eyre::eyre::bail!("This entry is not an editable native resource"),
            };
            saved_digest = Some(digest);
            Ok(warning)
        })();
        match result {
            Ok(warning) => {
                if let (Some(editor), Some(digest)) =
                    (self.ui.resource_editor.as_mut(), saved_digest)
                {
                    editor.saved(digest);
                }
                self.refresh_artifact_sidebar_catalogs();
                self.ui
                    .show_toast(warning.unwrap_or_else(|| "Entry saved.".into()));
            }
            Err(error) => self.show_resource_error(Self::artifact_domain(&handle), error),
        }
    }

    pub(crate) fn delete_artifact(&mut self, handle: crate::ui::artifact_sidebar::ArtifactHandle) {
        if let Err(error) = self.require_artifact_access(&handle) {
            self.show_resource_error(Self::artifact_domain(&handle), error);
            return;
        }
        let deleted_path = self
            .find_artifact(&handle)
            .and_then(|artifact| artifact.path);
        let result: color_eyre::Result<()> = match &handle {
            crate::ui::artifact_sidebar::ArtifactHandle::ChatFile { .. } => {
                self.delete_canonical_file(&handle);
                return;
            }
            crate::ui::artifact_sidebar::ArtifactHandle::Memory(id) => {
                #[cfg(feature = "memory")]
                {
                    crate::memory::MemoryStore::open_default()
                        .and_then(|store| store.trash_entry(*id))
                        .map_err(Into::into)
                }
                #[cfg(not(feature = "memory"))]
                {
                    let _ = id;
                    Err(color_eyre::eyre::eyre!(
                        "Memories are not enabled in this build"
                    ))
                }
            }
            crate::ui::artifact_sidebar::ArtifactHandle::Vault(id) => {
                self.chat_files.trash_item(*id).map_err(Into::into)
            }
            _ => {
                self.remove_temporary_artifact(&handle);
                Ok(())
            }
        };
        if let Err(error) = result {
            #[cfg(feature = "memory")]
            let removed = error.chain().any(|cause| {
                cause
                    .downcast_ref::<crate::memory::MemoryError>()
                    .is_some_and(|error| {
                        matches!(
                            error,
                            crate::memory::MemoryError::RemovedButNotIndexed { .. }
                        )
                    })
            });
            #[cfg(not(feature = "memory"))]
            let removed = false;
            if !removed {
                self.show_resource_error(Self::artifact_domain(&handle), error);
                return;
            }
            self.ui.show_toast(error.to_string());
        }
        self.refresh_artifact_sidebar_catalogs();
        let conversations: Vec<_> = self
            .ui
            .tabs
            .iter()
            .enumerate()
            .map(|(index, tab)| (index, tab.active_conversation))
            .collect();
        for (index, conversation) in conversations {
            if let Err(error) = self.hydrate_chat_files(index, conversation) {
                self.ui.show_toast(format!(
                    "Item removed; chat artifacts need refresh: {error}"
                ));
            }
        }
        if self.ui.artifact_viewer.as_ref().is_some_and(|viewer| {
            viewer.handle() == &handle
                || deleted_path
                    .as_ref()
                    .is_some_and(|path| viewer.artifact.path.as_ref() == Some(path))
        }) {
            self.ui.artifact_viewer = None;
        }
    }

    pub(crate) fn remove_temporary_artifact(
        &mut self,
        handle: &crate::ui::artifact_sidebar::ArtifactHandle,
    ) {
        if let Some(tab) = self.ui.tabs.get_mut(self.ui.active_tab) {
            tab.temporary_artifacts
                .retain(|artifact| &artifact.handle != handle);
        }
    }

    pub(crate) fn find_artifact(
        &self,
        handle: &crate::ui::artifact_sidebar::ArtifactHandle,
    ) -> Option<crate::ui::artifact_sidebar::ArtifactEntry> {
        self.require_artifact_access(handle).ok()?;
        self.ui
            .tabs
            .get(self.ui.active_tab)
            .and_then(|tab| {
                tab.temporary_artifacts
                    .iter()
                    .find(|artifact| &artifact.handle == handle)
            })
            .or_else(|| {
                self.ui
                    .memory_artifacts
                    .iter()
                    .find(|artifact| &artifact.handle == handle)
            })
            .or_else(|| {
                self.ui
                    .vault_artifacts
                    .iter()
                    .find(|artifact| &artifact.handle == handle)
            })
            .cloned()
    }

    pub(crate) fn copy_resource_editor(&mut self) {
        let result = (|| -> color_eyre::Result<()> {
            let editor = self
                .ui
                .resource_editor
                .as_ref()
                .ok_or_else(|| color_eyre::eyre::eyre!("No native entry editor is open"))?;
            self.require_artifact_access(&editor.handle)?;
            crate::clipboard::copy(editor.text())?;
            Ok(())
        })();
        match result {
            Ok(()) => self
                .ui
                .show_toast("Editor content sent to the clipboard as plaintext.".into()),
            Err(error) => self.ui.show_toast(error.to_string()),
        }
    }

    #[cfg(feature = "memory")]
    pub(crate) fn open_memories_launcher(&mut self) {
        self.ui
            .app_tabs
            .open_view(crate::ui::app_tabs::AppView::Memories);
        if let Err(error) = self.refresh_memory_artifacts() {
            self.ui.memory_notice = Some(error.to_string());
            self.show_resource_error(crate::protection::Domain::Memories, error);
        }
    }

    #[cfg(feature = "memory")]
    pub(crate) fn handle_memories_key(
        &mut self,
        key: crossterm::event::KeyEvent,
    ) -> Option<super::Action> {
        use crossterm::event::{KeyCode, KeyModifiers};
        if key.modifiers.contains(KeyModifiers::CONTROL) {
            return match key.code {
                KeyCode::Char('w') => {
                    self.request_app_tab_close(self.ui.app_tabs.active_id());
                    None
                }
                KeyCode::Char('q') => Some(self.quit_action()),
                _ => None,
            };
        }
        let action = self.ui.memory_manager.key(key, &self.ui.memory_artifacts)?;
        self.apply_memory_manager_action(action)
    }

    #[cfg(feature = "memory")]
    pub(crate) fn handle_memories_mouse(
        &mut self,
        position: ratatui::layout::Position,
    ) -> Option<super::Action> {
        let action = self
            .ui
            .memory_manager
            .hit_areas
            .iter()
            .find_map(|(area, action)| area.contains(position).then_some(*action))?;
        self.apply_memory_manager_action(action)
    }

    #[cfg(feature = "memory")]
    fn apply_memory_manager_action(
        &mut self,
        action: crate::ui::memory_manager::MemoryManagerAction,
    ) -> Option<super::Action> {
        use crate::ui::memory_manager::MemoryManagerAction;
        match action {
            MemoryManagerAction::Select(index) => {
                self.ui
                    .memory_manager
                    .select(index, &self.ui.memory_artifacts);
                return None;
            }
            MemoryManagerAction::Transfer => {
                return Some(super::Action::OpenNativeTransfer(
                    crate::protection::Domain::Memories,
                ));
            }
            MemoryManagerAction::Refresh => {
                self.open_memories_launcher();
                return None;
            }
            _ => {}
        }
        let Some(handle) = self.ui.memory_manager.selected(&self.ui.memory_artifacts) else {
            self.ui.show_toast("Select a memory first.".into());
            return None;
        };
        match action {
            MemoryManagerAction::View => self.open_artifact(handle),
            MemoryManagerAction::Edit => self.edit_artifact(handle),
            MemoryManagerAction::Copy => self.copy_artifact(handle),
            MemoryManagerAction::Delete => self.ui.delete_confirm = Some(handle),
            MemoryManagerAction::Context => self.insert_artifact_to_chat(handle),
            MemoryManagerAction::Select(_)
            | MemoryManagerAction::Transfer
            | MemoryManagerAction::Refresh => unreachable!("handled above"),
        }
        None
    }
}

pub(crate) fn local_media_sources(content: &str) -> Vec<String> {
    let mut sources = Vec::new();
    let mut seen = HashSet::new();

    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("![") && trimmed.ends_with(')') {
            if let Some(start) = trimmed.find("](") {
                let source = trimmed[start + 2..trimmed.len().saturating_sub(1)]
                    .trim()
                    .trim_matches('<')
                    .trim_matches('>');
                if is_artifact_image(source) && seen.insert(source.to_string()) {
                    sources.push(source.to_string());
                }
            }
            continue;
        }

        if is_artifact_image(trimmed) && seen.insert(trimmed.to_string()) {
            sources.push(trimmed.to_string());
        }
    }

    sources
}

fn is_artifact_image(source: &str) -> bool {
    if !crate::ui::components::image_block::is_local_image_source(source) {
        return false;
    }
    let trimmed = source
        .trim()
        .trim_matches('<')
        .trim_matches('>')
        .strip_prefix("file://")
        .unwrap_or(source.trim());
    matches!(
        std::path::Path::new(trimmed)
            .extension()
            .and_then(|extension| extension.to_str())
            .map(str::to_ascii_lowercase)
            .as_deref(),
        Some("png" | "jpg" | "jpeg" | "gif" | "webp" | "bmp")
    )
}
