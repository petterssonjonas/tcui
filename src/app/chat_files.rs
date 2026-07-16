use super::TuiApp;

impl TuiApp {
    pub(crate) fn persist_generated_file(
        &mut self,
        tab_id: usize,
        mut file: crate::app::GeneratedFile,
    ) -> color_eyre::Result<()> {
        let record =
            self.chat_files
                .create(file.conversation_id, &file.name, file.content.as_bytes())?;
        file.name = record.name;
        file.chat_file_id = Some(record.id);
        self.ui.add_generated_file(tab_id, file);
        Ok(())
    }

    pub(crate) fn hydrate_chat_files(
        &mut self,
        tab_id: usize,
        conversation_id: i64,
    ) -> color_eyre::Result<()> {
        let records = self.chat_files.list(conversation_id)?;
        let mut artifacts = Vec::with_capacity(records.len());
        let mut skipped = 0_usize;
        for record in records {
            let content = match self
                .chat_files
                .open(conversation_id, record.id)
                .ok()
                .and_then(|bytes| String::from_utf8(bytes).ok())
            {
                Some(content) => content,
                None => {
                    skipped += 1;
                    continue;
                }
            };
            artifacts.push(crate::ui::artifact_sidebar::ArtifactEntry::chat_file(
                conversation_id,
                record.id,
                record.name,
                content,
            ));
        }
        if let Some(tab) = self.ui.tabs.get_mut(tab_id) {
            tab.temporary_artifacts = artifacts;
        }
        if skipped > 0 {
            self.ui
                .show_toast("Some chat files could not be loaded.".to_string());
        }
        Ok(())
    }

    pub(crate) fn save_chat_file_copy(
        &mut self,
        artifact: &crate::ui::artifact_sidebar::ArtifactEntry,
        conversation_id: i64,
        id: crate::storage::chat_files::ChatFileId,
    ) {
        let saved = self
            .chat_files
            .open(conversation_id, id)
            .map_err(color_eyre::Report::from)
            .and_then(|bytes| String::from_utf8(bytes).map_err(color_eyre::Report::from))
            .and_then(|content| {
                crate::app::GeneratedFile {
                    id: rand::random(),
                    conversation_id,
                    name: artifact.name.clone(),
                    content,
                    saved_path: None,
                    chat_file_id: Some(id),
                }
                .save_to(&self.export_base_dir())
                .map_err(color_eyre::Report::from)
            });
        match saved {
            Ok(saved) => {
                self.refresh_saved_artifacts();
                self.ui.connection_status = crate::ui::status_bar::ConnectionStatus::CloudConnected;
                self.ui.connection_message = saved
                    .saved_path
                    .map(|path| format!("Saved {}", path.display()));
            }
            Err(error) => {
                self.ui.connection_status = crate::ui::status_bar::ConnectionStatus::Failed;
                self.ui.connection_message = Some(format!("Chat file save failed: {error}"));
            }
        }
    }

    pub(crate) fn delete_chat_file(
        &mut self,
        handle: &crate::ui::artifact_sidebar::ArtifactHandle,
        conversation_id: i64,
        id: crate::storage::chat_files::ChatFileId,
    ) {
        match self.chat_files.delete(conversation_id, id) {
            Ok(()) => self.remove_temporary_artifact(handle),
            Err(error) => {
                self.ui.connection_status = crate::ui::status_bar::ConnectionStatus::Failed;
                self.ui.connection_message = Some(format!("Chat file deletion failed: {error}"));
            }
        }
    }
}
