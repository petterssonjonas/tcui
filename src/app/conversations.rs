use super::TuiApp;

impl TuiApp {
    pub(crate) fn refresh_tab_conversations(&mut self, tab_id: usize) -> color_eyre::Result<()> {
        let (conversations, skipped) = self
            .storage
            .get_conversations_with_warnings(tab_id as i64)?;
        if let Some(tab) = self.ui.tabs.get_mut(tab_id) {
            tab.conversations = conversations
                .into_iter()
                .map(|conversation| crate::ui::ConversationEntry {
                    id: conversation.id,
                    title: conversation.title,
                    created_at: conversation.created_at,
                    updated_at_ms: conversation.updated_at_ms,
                    pinned: conversation.pinned,
                })
                .collect();
        }
        if skipped > 0 {
            self.ui
                .show_toast("Some chat history could not be loaded.".to_string());
        }
        Ok(())
    }

    pub(crate) fn ensure_tab_has_active_conversation(
        &mut self,
        tab_id: usize,
    ) -> color_eyre::Result<()> {
        self.refresh_tab_conversations(tab_id)?;
        let active_id = self.ui.tabs.get(tab_id).and_then(|tab| {
            tab.conversations
                .iter()
                .find(|conversation| conversation.id == tab.active_conversation)
                .map(|conversation| conversation.id)
                .or_else(|| {
                    tab.conversations
                        .first()
                        .map(|conversation| conversation.id)
                })
        });
        match active_id {
            Some(conversation_id) => self.load_conversation_into_tab(tab_id, conversation_id),
            None => self.new_conversation(tab_id),
        }
    }

    pub(crate) fn reset_tab_runtime_state(tab: &mut crate::ui::ChatTabState) {
        tab.messages.clear();
        tab.streaming = false;
        tab.thinking_fold_overrides.clear();
        tab.thinking_hit_areas.clear();
        tab.temporary_artifacts.clear();
        tab.scroll_offset = 0;
        tab.scroll_to_message = None;
        tab.input_content.clear();
        tab.input_cursor = 0;
        tab.input_scroll = 0;
        tab.input_history_index = None;
        tab.input_history_draft = None;
    }

    pub(crate) fn persist_tab_chat_selection(&self, tab_id: usize) -> color_eyre::Result<()> {
        let Some(tab) = self.ui.tabs.get(tab_id) else {
            return Ok(());
        };
        if tab.active_conversation == 0 {
            return Ok(());
        }
        self.storage.set_conversation_selection(
            tab.active_conversation,
            &crate::config::ChatSelection {
                provider: tab.tab.provider.clone(),
                model: tab.tab.model.clone(),
                effort: tab.tab.reasoning_effort.clone(),
            },
        )
    }

    pub(crate) fn load_conversation_into_tab(
        &mut self,
        tab_id: usize,
        conv_id: i64,
    ) -> color_eyre::Result<()> {
        self.cancel_escape = None;
        let document = self.storage.read_chat_document(conv_id)?;
        if self
            .ui
            .tabs
            .get(tab_id)
            .is_some_and(|tab| tab.active_conversation != conv_id)
        {
            self.persist_tab_chat_selection(tab_id)?;
        }
        let selection = document.selection;
        let messages = document.messages;
        let title = self.ui.tabs.get(tab_id).and_then(|tab| {
            tab.conversations
                .iter()
                .find(|conversation| conversation.id == conv_id)
                .map(|conversation| conversation.title.clone())
        });
        if let Some(tab) = self.ui.tabs.get_mut(tab_id) {
            Self::reset_tab_runtime_state(tab);
            tab.active_conversation = conv_id;
            tab.generated_title = title.filter(|value| value != "New Chat");
            tab.messages = messages;
            if let Some(selection) = &selection {
                tab.tab.provider = selection.provider.clone();
                tab.tab.model = selection.model.clone();
                tab.tab.reasoning_effort = selection.effort.clone();
                tab.tab.endpoint = None;
                tab.manual_model_override = true;
            }
        }
        if selection.is_none() {
            self.apply_default_chat_selection(tab_id);
        } else if tab_id == self.ui.active_tab {
            self.refresh_visible_selectors();
        }
        self.hydrate_chat_files(tab_id, conv_id)?;
        self.sync_request_view(tab_id);
        if tab_id == self.ui.active_tab {
            self.queue_connection_check_for_active_tab();
        }
        Ok(())
    }

    pub(crate) fn toggle_conversation_pinned(
        &mut self,
        conversation_id: i64,
    ) -> color_eyre::Result<()> {
        let pinned = self
            .ui
            .tabs
            .get(self.ui.active_tab)
            .and_then(|tab| {
                tab.conversations
                    .iter()
                    .find(|conversation| conversation.id == conversation_id)
                    .map(|conversation| conversation.pinned)
            })
            .unwrap_or(false);
        self.storage
            .set_conversation_pinned(conversation_id, !pinned)?;
        self.refresh_tab_conversations(self.ui.active_tab)?;
        Ok(())
    }

    pub(crate) fn delete_conversation_by_id(
        &mut self,
        conversation_id: i64,
    ) -> color_eyre::Result<()> {
        if self
            .ui
            .ephemeral
            .as_ref()
            .is_some_and(|pane| pane.conversation_id == conversation_id)
        {
            return self.close_ephemeral();
        }
        self.storage.delete_conversation(conversation_id)?;
        if let Err(error) = self.chat_files.remove_conversation_links(conversation_id) {
            let marker_error = self
                .chat_files
                .mark_orphaned_conversation(conversation_id)
                .err();
            self.ui.connection_status = crate::ui::status_bar::ConnectionStatus::Failed;
            self.ui.connection_message = Some(match marker_error {
                Some(marker_error) => format!(
                    "Conversation deleted; link cleanup failed ({error}) and retry marker could not be written ({marker_error})"
                ),
                None => format!(
                    "Conversation deleted; chat-file cleanup will retry at startup: {error}"
                ),
            });
        }
        self.recover_missing_request_destinations()?;
        let tab_id = self.ui.active_tab;
        let deleting_active = self
            .ui
            .tabs
            .get(tab_id)
            .map(|tab| tab.active_conversation == conversation_id)
            .unwrap_or(false);
        if deleting_active {
            self.ensure_tab_has_active_conversation(tab_id)?;
        } else {
            self.refresh_tab_conversations(tab_id)?;
        }
        Ok(())
    }

    pub(crate) fn new_conversation(&mut self, tab_id: usize) -> color_eyre::Result<()> {
        self.cancel_escape = None;
        let conv_id = self.storage.create_conversation(tab_id as i64)?;
        self.refresh_tab_conversations(tab_id)?;
        self.load_conversation_into_tab(tab_id, conv_id)?;
        self.persist_tab_chat_selection(tab_id)?;
        Ok(())
    }

    pub(crate) fn fork_active_conversation(&mut self) -> color_eyre::Result<()> {
        let tab_id = self.ui.active_tab;
        let tab = self
            .ui
            .tabs
            .get(tab_id)
            .ok_or_else(|| color_eyre::eyre::eyre!("No active chat to fork"))?;
        if !tab.input_content.is_empty()
            || tab
                .input_history_draft
                .as_ref()
                .is_some_and(|draft| !draft.is_empty())
            || !tab.pending_document_chips.is_empty()
            || self.has_pending_documents()
        {
            return Err(color_eyre::eyre::eyre!(
                "Send or clear the current draft and inserted documents before forking"
            ));
        }
        let source_id = tab.active_conversation;
        if source_id == 0 {
            return Err(color_eyre::eyre::eyre!("No active conversation to fork"));
        }
        let source_title = tab
            .conversations
            .iter()
            .find(|conversation| conversation.id == source_id)
            .map(|conversation| conversation.title.as_str())
            .or(tab.generated_title.as_deref())
            .unwrap_or("New Chat");
        let title = format!("Fork of {source_title}");
        let destination_tab = self.ui.main_chat_index();
        let selection =
            (destination_tab != tab_id).then(|| (tab.tab.clone(), tab.manual_model_override));
        if destination_tab != tab_id
            && (self.ui.tabs.get(destination_tab).is_some_and(|main| {
                !main.input_content.is_empty()
                    || main
                        .input_history_draft
                        .as_ref()
                        .is_some_and(|draft| !draft.is_empty())
                    || !main.pending_document_chips.is_empty()
            }) || self
                .pending_documents
                .get(&self.ui.app_tabs.active_id())
                .is_some_and(|documents| !documents.is_empty()))
        {
            return Err(color_eyre::eyre::eyre!(
                "Send or clear the main chat draft before opening a persistent fork"
            ));
        }
        self.persist_tab_chat_selection(tab_id)?;
        let messages = self.conversation_snapshot(source_id)?;
        let child =
            self.storage
                .fork_conversation(destination_tab as i64, source_id, &title, &messages)?;
        self.refresh_tab_conversations(destination_tab)?;
        self.load_conversation_into_tab(destination_tab, child.id)?;
        if let Some(tab) = self.ui.tabs.get_mut(destination_tab) {
            if let Some((selection, manual_model_override)) = selection {
                let owner_id = tab.tab.id;
                tab.tab = selection;
                tab.tab.id = owner_id;
                tab.manual_model_override = manual_model_override;
            }
            tab.sidebar_state
                .select_action(crate::ui::sidebar::SidebarAction::LoadConversation(
                    child.id,
                ));
        }
        self.persist_tab_chat_selection(destination_tab)?;
        self.focus_chat_pane(destination_tab);
        Ok(())
    }
}

#[cfg(test)]
#[path = "fork_tests.rs"]
mod fork_tests;
