use super::TuiApp;

impl TuiApp {
    pub(crate) fn finalize_assistant_message(
        &mut self,
        mut completion: crate::app::action::AssistantFinalization,
    ) -> color_eyre::Result<()> {
        if !self.request_accepts_events(completion.request_id) {
            return Ok(());
        }
        let Some(owner_index) = self.ensure_request_destination(completion.request_id)? else {
            return Ok(());
        };
        let Some(request) = self.requests.get(&completion.request_id) else {
            return Ok(());
        };
        let route = request.route;
        let message_idx = request.assistant_idx;
        completion.message.conversation_id = route.destination_conversation_id;
        completion.message.request = Some(route);
        let source_content = completion.message.content.clone();
        let import_media = match completion.generated_file {
            crate::app::action::GeneratedFileFinalization::NotRequested => {
                route.destination_conversation_id > 0
            }
            crate::app::action::GeneratedFileFinalization::Ready(mut file) => {
                file.conversation_id = route.destination_conversation_id;
                match self.persist_generated_file(owner_index, file) {
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
                owner_index,
                completion.message.conversation_id,
                &source_content,
            ) {
                self.ui.connection_status = crate::ui::status_bar::ConnectionStatus::Failed;
                self.ui.connection_message =
                    Some(format!("Generated media import failed: {error}"));
            }
        }
        if let Some(request) = self.requests.get_mut(&completion.request_id) {
            request.messages[message_idx] = completion.message;
            request.finalized = true;
            for tab in &mut self.ui.tabs {
                if tab.active_conversation == route.destination_conversation_id {
                    if let Some(existing) = tab.messages.get_mut(message_idx) {
                        existing.clone_from(&request.messages[message_idx]);
                    }
                    tab.streaming = false;
                    tab.scroll_to_message = Some(message_idx);
                }
            }
            self.storage
                .replace_messages(route.destination_conversation_id, &request.messages)?;
        }
        for index in 0..self.ui.tabs.len() {
            self.refresh_tab_conversations(index)?;
        }
        Ok(())
    }

    pub(crate) fn persist_generated_file(
        &mut self,
        tab_id: usize,
        file: crate::app::GeneratedFile,
    ) -> color_eyre::Result<String> {
        if file.conversation_id < 0 {
            let record = self
                .chat_files
                .create_saved(&file.name, file.content.as_bytes())?;
            if let Err(error) = self.refresh_canonical_file_catalogs(tab_id, file.conversation_id) {
                self.ui.show_toast(format!(
                    "Saved {}, but could not refresh the file list: {error}",
                    record.filename
                ));
            }
            return Ok(record.filename);
        }
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
        if conversation_id < 0 {
            if let Some(tab) = self.ui.tabs.get_mut(tab_id) {
                tab.temporary_artifacts.clear();
            }
            return Ok(());
        }
        self.ui.artifact_sidebar_state.clear_hit_areas();
        let links = self.chat_files.list_for_conversation(conversation_id)?;
        let artifacts = links
            .into_iter()
            .map(|link| {
                let path = self.chat_files.path_for_link(&link);
                crate::ui::artifact_sidebar::ArtifactEntry::chat_file(
                    conversation_id,
                    link.id,
                    link.filename,
                    path,
                )
            })
            .collect();
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
        for source in super::local_media_sources(content) {
            let Some(artifact) = crate::ui::artifact_sidebar::ArtifactEntry::temp_media(&source)
            else {
                continue;
            };
            let Some(path) = artifact.path else {
                continue;
            };
            if conversation_id < 0 {
                let bytes = std::fs::read(&path)?;
                let name = path
                    .file_name()
                    .and_then(|name| name.to_str())
                    .unwrap_or("saved-media");
                self.chat_files.create_saved(name, &bytes)?;
            } else {
                self.chat_files.import_for_chat(conversation_id, &path)?;
            }
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
        self.ui.artifact_sidebar_state.clear_hit_areas();
        let saved = self
            .chat_files
            .list_saved()?
            .into_iter()
            .map(|record| crate::ui::artifact_sidebar::ArtifactEntry::saved_file(record.path))
            .collect();
        if self
            .ui
            .tabs
            .get(tab_id)
            .is_some_and(|tab| tab.active_conversation == conversation_id)
        {
            self.hydrate_chat_files(tab_id, conversation_id)?;
        }
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
