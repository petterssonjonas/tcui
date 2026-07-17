use super::TuiApp;

impl TuiApp {
    pub(crate) fn finalize_assistant_message(
        &mut self,
        mut completion: crate::app::action::AssistantFinalization,
    ) -> color_eyre::Result<()> {
        let source_content = completion.message.content.clone();
        let import_media = match completion.generated_file {
            crate::app::action::GeneratedFileFinalization::NotRequested => true,
            crate::app::action::GeneratedFileFinalization::Ready(file) => {
                match self.persist_generated_file(completion.tab_id, file) {
                    Ok(filename) => {
                        completion.message.content = format!("Saved as {filename}");
                        self.ui.connection_status =
                            crate::ui::status_bar::ConnectionStatus::CloudConnected;
                        self.ui.connection_message = Some(completion.message.content.clone());
                        true
                    }
                    Err(error) => {
                        completion.message.content = "Could not save generated file.".to_string();
                        self.ui.connection_status = crate::ui::status_bar::ConnectionStatus::Failed;
                        self.ui.connection_message =
                            Some(format!("Generated file save failed: {error}"));
                        false
                    }
                }
            }
            crate::app::action::GeneratedFileFinalization::Invalid => {
                completion.message.content = "Could not save generated file.".to_string();
                self.ui.connection_status = crate::ui::status_bar::ConnectionStatus::Failed;
                self.ui.connection_message = Some("Generated file response was empty.".to_string());
                false
            }
        };
        if import_media {
            if let Err(error) = self.import_completed_assistant_media(
                completion.tab_id,
                completion.message.conversation_id,
                &source_content,
            ) {
                self.ui.connection_status = crate::ui::status_bar::ConnectionStatus::Failed;
                self.ui.connection_message =
                    Some(format!("Generated media import failed: {error}"));
            }
        }
        if let Some(tab) = self.ui.tabs.get_mut(completion.tab_id) {
            if let Some(existing) = tab.messages.get_mut(completion.message_idx) {
                *existing = completion.message;
            }
            tab.scroll_to_message = Some(completion.message_idx);
        }
        self.persist_active_conversation(completion.tab_id)?;
        self.refresh_tab_conversations(completion.tab_id)
    }

    pub(crate) fn persist_generated_file(
        &mut self,
        tab_id: usize,
        file: crate::app::GeneratedFile,
    ) -> color_eyre::Result<String> {
        let link = self.chat_files.create_for_chat(
            file.conversation_id,
            &file.name,
            file.content.as_bytes(),
        )?;
        if let Err(refresh_error) =
            self.refresh_canonical_file_catalogs(tab_id, file.conversation_id)
        {
            return match self
                .chat_files
                .delete_by_link(file.conversation_id, link.id)
            {
                Ok(()) => Err(refresh_error),
                Err(rollback_error) => Err(color_eyre::eyre::eyre!(
                    "generated file catalog refresh failed ({refresh_error}); rollback failed ({rollback_error})"
                )),
            };
        }
        Ok(link.filename)
    }

    pub(crate) fn hydrate_chat_files(
        &mut self,
        tab_id: usize,
        conversation_id: i64,
    ) -> color_eyre::Result<()> {
        let links = self.chat_files.list_for_conversation(conversation_id)?;
        let mut artifacts = Vec::with_capacity(links.len());
        for link in links {
            let bytes = self.chat_files.open_link(conversation_id, link.id)?;
            let path = self.chat_files.path_for_link(&link);
            let path_source = path.to_string_lossy();
            let mut artifact = crate::ui::artifact_sidebar::ArtifactEntry::temp_media(&path_source)
                .ok_or_else(|| color_eyre::eyre::eyre!("saved file path is unavailable"))?;
            artifact.handle = crate::ui::artifact_sidebar::ArtifactHandle::ChatFile {
                conversation_id,
                id: link.id,
            };
            artifact.name = link.filename;
            artifact.content = String::from_utf8(bytes).ok();
            artifacts.push(artifact);
        }
        if let Some(tab) = self.ui.tabs.get_mut(tab_id) {
            tab.temporary_artifacts = artifacts;
        }
        Ok(())
    }

    pub(crate) fn import_completed_assistant_media(
        &mut self,
        tab_id: usize,
        conversation_id: i64,
        content: &str,
    ) -> color_eyre::Result<()> {
        let mut imported = false;
        for source in crate::app::local_media_sources(content) {
            let Some(artifact) = crate::ui::artifact_sidebar::ArtifactEntry::temp_media(&source)
            else {
                continue;
            };
            let Some(path) = artifact.path else {
                continue;
            };
            self.chat_files.import_for_chat(conversation_id, &path)?;
            imported = true;
        }
        if imported {
            self.refresh_canonical_file_catalogs(tab_id, conversation_id)?;
        }
        Ok(())
    }

    pub(crate) fn refresh_canonical_file_catalogs(
        &mut self,
        tab_id: usize,
        conversation_id: i64,
    ) -> color_eyre::Result<()> {
        let saved = self
            .chat_files
            .list_saved()?
            .into_iter()
            .map(|record| crate::ui::artifact_sidebar::ArtifactEntry::saved_file(record.path))
            .collect();
        self.hydrate_chat_files(tab_id, conversation_id)?;
        self.ui.saved_artifacts = saved;
        Ok(())
    }

    pub(crate) fn delete_canonical_file(
        &mut self,
        handle: &crate::ui::artifact_sidebar::ArtifactHandle,
    ) {
        let deleted_path = self
            .find_artifact(handle)
            .and_then(|artifact| artifact.path)
            .or_else(|| match handle {
                crate::ui::artifact_sidebar::ArtifactHandle::Saved(path) => Some(path.clone()),
                _ => None,
            });
        let deletion = match handle {
            crate::ui::artifact_sidebar::ArtifactHandle::ChatFile {
                conversation_id,
                id,
            } => self.chat_files.delete_by_link(*conversation_id, *id),
            crate::ui::artifact_sidebar::ArtifactHandle::Saved(path) => {
                self.chat_files.delete_by_saved_path(path)
            }
            _ => return,
        };
        if let Err(error) = deletion {
            self.ui.connection_status = crate::ui::status_bar::ConnectionStatus::Failed;
            self.ui.connection_message = Some(format!("Saved file deletion failed: {error}"));
            return;
        }
        let tab_id = self.ui.active_tab;
        let conversation_id = self
            .ui
            .tabs
            .get(tab_id)
            .map(|tab| tab.active_conversation)
            .unwrap_or(0);
        if let Err(error) = self.refresh_canonical_file_catalogs(tab_id, conversation_id) {
            self.ui.connection_status = crate::ui::status_bar::ConnectionStatus::Failed;
            self.ui.connection_message = Some(format!("File catalogs could not refresh: {error}"));
        }
        if self.ui.artifact_viewer.as_ref().is_some_and(|viewer| {
            viewer.handle() == handle
                || deleted_path
                    .as_ref()
                    .is_some_and(|path| viewer.artifact.path.as_ref() == Some(path))
        }) {
            self.ui.artifact_viewer = None;
        }
    }
}
