use super::TuiApp;

impl TuiApp {
    pub(crate) fn save_generated_file(&mut self) -> color_eyre::Result<()> {
        let Some(dialog) = self.ui.save_file_dialog.clone() else {
            return Ok(());
        };

        let trimmed_path = dialog.path_input.trim();
        if trimmed_path.is_empty() {
            self.ui.connection_status = crate::ui::status_bar::ConnectionStatus::Failed;
            self.ui.connection_message = Some("Choose a path before saving.".to_string());
            return Ok(());
        }

        let path = crate::app::generated_file::expand_user_path(
            std::path::Path::new(trimmed_path),
            dirs::home_dir().as_deref(),
        );
        if let Some(parent) = path
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
        {
            std::fs::create_dir_all(parent)?;
        }
        match &dialog.artifact.content {
            Some(content) => std::fs::write(&path, content.as_bytes())?,
            None => {
                let Some(source) = &dialog.artifact.path else {
                    return Ok(());
                };
                std::fs::copy(source, &path)?;
            }
        }

        self.ui.save_file_dialog = None;
        self.promote_temporary_artifact_if_vault_path(&dialog.artifact.handle, &path);
        self.refresh_saved_artifacts();
        self.ui.connection_status = crate::ui::status_bar::ConnectionStatus::CloudConnected;
        self.ui.connection_message = Some(format!("Saved {}", path.display()));
        Ok(())
    }

    pub(crate) fn export_base_dir(&self) -> std::path::PathBuf {
        self.config
            .try_read()
            .ok()
            .and_then(|cfg| cfg.artifact_save_dir.clone())
            .map(|path| {
                crate::app::generated_file::expand_user_path(
                    std::path::Path::new(&path),
                    dirs::home_dir().as_deref(),
                )
            })
            .or_else(dirs::download_dir)
            .or_else(|| std::env::current_dir().ok())
            .unwrap_or_else(|| std::path::PathBuf::from("."))
    }

    pub(crate) fn open_conversation_export_dialog(&mut self) {
        let Some(tab) = self.ui.tabs.get(self.ui.active_tab) else {
            return;
        };
        if tab.active_conversation == 0 {
            return;
        }
        self.open_conversation_export_dialog_for(tab.active_conversation);
    }

    pub(crate) fn open_conversation_export_dialog_for(&mut self, conversation_id: i64) {
        let item_name = self
            .ui
            .tabs
            .get(self.ui.active_tab)
            .and_then(|tab| {
                tab.conversations
                    .iter()
                    .find(|conversation| conversation.id == conversation_id)
                    .map(|conversation| conversation.title.clone())
                    .or_else(|| tab.generated_title.clone())
            })
            .unwrap_or_else(|| "New Chat".to_string());
        self.ui.export_dialog = Some(crate::ui::modals::export_dialog::ExportDialog::new(
            crate::ui::modals::export_dialog::ExportTarget::Conversation(conversation_id),
            item_name,
            self.export_base_dir(),
        ));
    }

    pub(crate) fn open_memory_export_dialog(&mut self, logical_path: std::path::PathBuf) {
        #[cfg(feature = "memory")]
        {
            let item_name = self
                .ui
                .memory_artifacts
                .iter()
                .find_map(|artifact| match &artifact.handle {
                    crate::ui::artifact_sidebar::ArtifactHandle::Memory(path)
                        if path == &logical_path =>
                    {
                        Some(artifact.name.clone())
                    }
                    _ => None,
                })
                .unwrap_or_else(|| logical_path.display().to_string());
            self.ui.export_dialog = Some(crate::ui::modals::export_dialog::ExportDialog::new(
                crate::ui::modals::export_dialog::ExportTarget::Memory(logical_path),
                item_name,
                self.export_base_dir(),
            ));
        }
        #[cfg(not(feature = "memory"))]
        let _ = logical_path;
    }

    pub(crate) fn save_export_dialog(&mut self) -> color_eyre::Result<()> {
        let Some(dialog) = self.ui.export_dialog.clone() else {
            return Ok(());
        };
        let trimmed_dir = dialog.directory_input.trim();
        if trimmed_dir.is_empty() {
            self.ui.connection_status = crate::ui::status_bar::ConnectionStatus::Failed;
            self.ui.connection_message = Some("Choose an export directory.".to_string());
            return Ok(());
        }
        let destination = crate::app::generated_file::expand_user_path(
            std::path::Path::new(trimmed_dir),
            dirs::home_dir().as_deref(),
        );
        let saved = match dialog.target {
            crate::ui::modals::export_dialog::ExportTarget::Conversation(conversation_id) => {
                let document = self
                    .storage
                    .list_all_chat_documents()?
                    .into_iter()
                    .find(|document| document.id == conversation_id)
                    .ok_or_else(|| color_eyre::eyre::eyre!("conversation is unavailable"))?;
                crate::export::export_chat_document_to_dir(&document, dialog.format, &destination)?
            }
            #[cfg(feature = "memory")]
            crate::ui::modals::export_dialog::ExportTarget::Memory(path) => {
                let config = self.config.blocking_read().clone();
                let vault = config
                    .vault_path
                    .as_deref()
                    .map(std::path::Path::new)
                    .ok_or_else(|| color_eyre::eyre::eyre!("Obsidian vault is not configured"))?;
                let store = crate::memory::MemoryStore::open(
                    vault,
                    &crate::memory::MemoryStore::default_cache_path(),
                )?;
                let document = store
                    .find_document_by_logical_path(&path)?
                    .ok_or_else(|| color_eyre::eyre::eyre!("memory is unavailable"))?;
                crate::export::export_memory_document_to_dir(
                    &document,
                    dialog.format,
                    &destination,
                )?
            }
        };
        self.ui.export_dialog = None;
        self.ui.show_toast(format!("Exported {}", saved.display()));
        Ok(())
    }
}
