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

    pub(crate) fn load_conversation_into_tab(
        &mut self,
        tab_id: usize,
        conv_id: i64,
    ) -> color_eyre::Result<()> {
        let messages = self.storage.get_messages(conv_id)?;
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
        }
        self.sync_message_media(tab_id);
        Ok(())
    }

    pub(crate) fn persist_active_conversation(&self, tab_id: usize) -> color_eyre::Result<()> {
        let Some(tab) = self.ui.tabs.get(tab_id) else {
            return Ok(());
        };
        if tab.active_conversation == 0 {
            return Ok(());
        }
        self.storage
            .replace_messages(tab.active_conversation, &tab.messages)?;
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
        self.storage.delete_conversation(conversation_id)?;
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
        let conv_id = self.storage.create_conversation(tab_id as i64)?;
        self.refresh_tab_conversations(tab_id)?;
        self.load_conversation_into_tab(tab_id, conv_id)
    }
}
