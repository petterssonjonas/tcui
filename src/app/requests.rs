use color_eyre::eyre::{eyre, Result};
use serde::{Deserialize, Serialize};
use tokio::task::AbortHandle;

use super::{submission::SubmissionId, Message, Tab, TuiApp};
use crate::ui::app_tabs::AppTabId;

/// The origin is provenance, not a UI address. Only the destination receives output.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct RequestRoute {
    pub(crate) request_id: SubmissionId,
    pub(crate) origin_conversation_id: i64,
    pub(crate) destination_conversation_id: i64,
}

pub(crate) struct ConversationRequest {
    pub(crate) route: RequestRoute,
    pub(crate) app_tab_id: AppTabId,
    pub(crate) owner: Tab,
    pub(crate) messages: Vec<Message>,
    pub(crate) assistant_idx: usize,
    pub(crate) title: String,
    pub(crate) abort: Option<AbortHandle>,
    pub(crate) finalized: bool,
    pub(crate) cancelled: bool,
    pub(crate) hide_answer: bool,
}

impl Drop for ConversationRequest {
    fn drop(&mut self) {
        if let Some(handle) = &self.abort {
            handle.abort();
        }
    }
}

/// Also runs when a provider future fails or is dropped. Late notifications are harmless.
pub(crate) struct StreamCompletion {
    pub(crate) request_id: SubmissionId,
    pub(crate) sender: tokio::sync::mpsc::UnboundedSender<super::Action>,
}

impl Drop for StreamCompletion {
    fn drop(&mut self) {
        let _ = self.sender.send(super::Action::StopStream(self.request_id));
    }
}

impl TuiApp {
    pub(crate) fn active_request_id(&self) -> Option<SubmissionId> {
        if !self.ui.app_tabs.active_view().is_chat() {
            return None;
        }
        let conversation = self.ui.tabs.get(self.ui.active_tab)?.active_conversation;
        self.requests.values().find_map(|request| {
            (request.route.destination_conversation_id == conversation)
                .then_some(request.route.request_id)
        })
    }

    pub(crate) fn conversation_snapshot(&self, conversation_id: i64) -> Result<Vec<Message>> {
        if let Some(request) = self
            .requests
            .values()
            .find(|request| request.route.destination_conversation_id == conversation_id)
        {
            return Ok(request.messages.clone());
        }
        self.storage.get_messages(conversation_id)
    }

    /// Register a destination snapshot before launching work. Callers can route work from
    /// another conversation without making a UI selection or creating a second execution.
    pub(crate) fn register_request(
        &mut self,
        mut route: RequestRoute,
        owner_index: usize,
        mut messages: Vec<Message>,
    ) -> Result<()> {
        if route.origin_conversation_id == 0 || route.destination_conversation_id == 0 {
            return Err(eyre!(
                "A request requires valid origin and destination conversations"
            ));
        }
        if self.requests.contains_key(&route.request_id)
            || self.requests.values().any(|request| {
                request.route.destination_conversation_id == route.destination_conversation_id
            })
        {
            return Err(eyre!(
                "This conversation already has an active request. Use /cancel first."
            ));
        }
        let tab = self
            .ui
            .tabs
            .get_mut(owner_index)
            .ok_or_else(|| eyre!("The request's chat tab no longer exists"))?;
        if tab.tab.id.is_none() {
            tab.tab.id = Some((rand::random::<u64>() & i64::MAX as u64) as i64);
        }
        let owner = tab.tab.clone();
        let app_tab_id = self
            .ui
            .app_tabs
            .tabs()
            .iter()
            .find(|tab| tab.view().is_chat())
            .map(|tab| tab.id())
            .ok_or_else(|| eyre!("No chat workspace is available for this request"))?;
        let original_destination = route.destination_conversation_id;
        let mut recovered = false;
        let title = match self.storage.read_chat_document(original_destination) {
            Ok(document) => document.title,
            Err(error) => {
                if original_destination < 0 {
                    return Err(error);
                }
                if self.storage.conversation_exists(original_destination)? {
                    return Err(error);
                }
                let title = "Recovered conversation".to_string();
                route.destination_conversation_id = self.storage.create_conversation_snapshot(
                    owner_index as i64,
                    &title,
                    &messages,
                )?;
                recovered = true;
                title
            }
        };
        for message in &mut messages {
            message.conversation_id = route.destination_conversation_id;
        }
        if !recovered {
            self.storage
                .replace_messages(route.destination_conversation_id, &messages)?;
        }
        let assistant_idx = messages.len();
        let mut assistant = Message::new(
            route.destination_conversation_id,
            "assistant".into(),
            String::new(),
        );
        assistant.request = Some(route);
        messages.push(assistant);
        self.requests.insert(
            route.request_id,
            ConversationRequest {
                route,
                app_tab_id,
                owner,
                messages,
                assistant_idx,
                title,
                abort: None,
                finalized: false,
                cancelled: false,
                hide_answer: false,
            },
        );
        self.cancel_escape = None;
        for index in 0..self.ui.tabs.len() {
            if recovered && self.ui.tabs[index].active_conversation == original_destination {
                self.ui.tabs[index].active_conversation = route.destination_conversation_id;
            }
            if recovered {
                self.refresh_tab_conversations(index)?;
            }
            if self.ui.tabs[index].active_conversation == route.destination_conversation_id {
                self.sync_request_view(index);
            }
        }
        Ok(())
    }

    pub(crate) fn sync_request_view(&mut self, tab_index: usize) {
        let Some(tab) = self.ui.tabs.get_mut(tab_index) else {
            return;
        };
        let request = self
            .requests
            .values()
            .find(|request| request.route.destination_conversation_id == tab.active_conversation);
        if let Some(request) = request {
            tab.messages.clone_from(&request.messages);
            tab.generated_title = Some(request.title.clone());
            tab.streaming = !request.finalized && !request.cancelled;
            if request.hide_answer && tab.streaming {
                tab.messages[request.assistant_idx].content.clear();
            }
        } else {
            tab.streaming = false;
        }
    }

    pub(crate) fn request_accepts_events(&self, id: SubmissionId) -> bool {
        self.requests
            .get(&id)
            .is_some_and(|request| !request.finalized && !request.cancelled)
    }

    /// Recover only absence. Permission, decryption, and other storage errors must surface.
    pub(crate) fn ensure_request_destination(&mut self, id: SubmissionId) -> Result<Option<usize>> {
        let Some(request) = self.requests.get(&id) else {
            return Ok(None);
        };
        let owner_index = self
            .ui
            .tabs
            .iter()
            .position(|tab| tab.tab.id == request.owner.id);
        let tab_exists = self.ui.app_tabs.tab(request.app_tab_id).is_some();
        if let Some(index) = owner_index.filter(|_| tab_exists) {
            if self
                .storage
                .conversation_exists(request.route.destination_conversation_id)?
            {
                return Ok(Some(index));
            }
        }
        if request.route.destination_conversation_id < 0 {
            return Err(eyre!("The temporary conversation has been closed"));
        }
        let old_destination = request.route.destination_conversation_id;
        let title = format!(
            "Recovered: {}",
            request.title.trim_start_matches("Recovered: ")
        );
        let owner_index = match owner_index {
            Some(index) => index,
            None => {
                if self.ui.tabs.is_empty() {
                    self.ui.add_tab(self.requests[&id].owner.clone());
                }
                self.ui.active_tab.min(self.ui.tabs.len() - 1)
            }
        };
        if self.ui.tabs[owner_index].tab.id.is_none() {
            self.ui.tabs[owner_index].tab.id =
                Some((rand::random::<u64>() & i64::MAX as u64) as i64);
        }
        let request = self
            .requests
            .get(&id)
            .ok_or_else(|| eyre!("Request disappeared during recovery"))?;
        let destination = self.storage.create_conversation_snapshot(
            owner_index as i64,
            &title,
            &request.messages,
        )?;
        let request = self
            .requests
            .get_mut(&id)
            .ok_or_else(|| eyre!("Request disappeared during recovery"))?;
        request.route.destination_conversation_id = destination;
        request.owner = self.ui.tabs[owner_index].tab.clone();
        request.title = title;
        request.app_tab_id = self
            .ui
            .app_tabs
            .tabs()
            .iter()
            .find(|tab| tab.view().is_chat())
            .map_or(request.app_tab_id, |tab| tab.id());
        for message in &mut request.messages {
            message.conversation_id = destination;
            if message.request.is_some_and(|route| route.request_id == id) {
                message.request = Some(request.route);
            }
        }
        // Publish the new destination before a second write. A retry must not create another chat.
        self.storage
            .replace_messages(destination, &request.messages)?;
        self.cancel_escape = None;
        for tab in &mut self.ui.tabs {
            if tab.active_conversation == old_destination {
                tab.active_conversation = destination;
            }
        }
        for index in 0..self.ui.tabs.len() {
            self.refresh_tab_conversations(index)?;
            self.sync_request_view(index);
        }
        self.ui.show_toast(
            "The stream's destination was deleted. Output continues in a recovered conversation."
                .into(),
        );
        Ok(Some(owner_index))
    }

    pub(crate) fn recover_missing_request_destinations(&mut self) -> Result<()> {
        let ids: Vec<_> = self.requests.keys().copied().collect();
        for id in ids {
            self.ensure_request_destination(id)?;
        }
        Ok(())
    }

    pub(crate) fn append_request_content(
        &mut self,
        id: SubmissionId,
        content: &str,
        thinking: bool,
    ) -> Result<()> {
        if !self.request_accepts_events(id) {
            return Ok(());
        }
        let Some(request) = self.requests.get_mut(&id) else {
            return Ok(());
        };
        let assistant = &mut request.messages[request.assistant_idx];
        if thinking {
            assistant
                .thinking_content
                .get_or_insert_with(String::new)
                .push_str(content);
        } else {
            assistant.content.push_str(content);
        }
        if request.hide_answer && !thinking {
            return Ok(());
        }
        for tab in &mut self.ui.tabs {
            if tab.active_conversation != request.route.destination_conversation_id {
                continue;
            }
            if let Some(message) = tab
                .messages
                .get_mut(request.assistant_idx)
                .filter(|message| message.request.is_some_and(|route| route.request_id == id))
            {
                if thinking {
                    message
                        .thinking_content
                        .get_or_insert_with(String::new)
                        .push_str(content);
                } else {
                    message.content.push_str(content);
                }
                tab.scroll_to_message = Some(request.assistant_idx);
            }
        }
        Ok(())
    }

    #[cfg(feature = "memory")]
    pub(crate) fn set_request_memory_activities(
        &mut self,
        id: SubmissionId,
        activities: &[crate::memory::MemoryActivity],
    ) -> Result<()> {
        if !self.request_accepts_events(id) {
            return Ok(());
        }
        let Some(request) = self.requests.get_mut(&id) else {
            return Ok(());
        };
        crate::memory::set_activities(&mut request.messages[request.assistant_idx], activities)?;
        for tab in &mut self.ui.tabs {
            if tab.active_conversation != request.route.destination_conversation_id {
                continue;
            }
            if let Some(message) = tab
                .messages
                .get_mut(request.assistant_idx)
                .filter(|message| message.request.is_some_and(|route| route.request_id == id))
            {
                crate::memory::set_activities(message, activities)?;
            }
        }
        Ok(())
    }

    pub(crate) fn finish_request(&mut self, id: SubmissionId) -> Result<()> {
        if !self.requests.contains_key(&id) {
            return Ok(());
        }
        self.ensure_request_destination(id)?;
        if let Some(request) = self.requests.get_mut(&id) {
            if !request.finalized && !request.cancelled {
                request.finalized = true;
                let message = &mut request.messages[request.assistant_idx];
                if !message.content.is_empty() {
                    message.content.push_str("\n\n");
                }
                message.content.push_str("[Response interrupted]");
            }
        }
        for index in 0..self.ui.tabs.len() {
            if self.ui.tabs[index].active_conversation
                == self.requests[&id].route.destination_conversation_id
            {
                self.sync_request_view(index);
            }
        }
        let request = self
            .requests
            .get(&id)
            .ok_or_else(|| eyre!("Request disappeared before persistence"))?;
        self.storage
            .replace_messages(request.route.destination_conversation_id, &request.messages)?;
        let destination = request.route.destination_conversation_id;
        if let Some(mut request) = self.requests.remove(&id) {
            // Completed work can finish notification delivery; cancelled work is aborted explicitly.
            request.abort = None;
        }
        self.cancel_escape = None;
        for index in 0..self.ui.tabs.len() {
            if self.ui.tabs[index].active_conversation == destination {
                self.ui.tabs[index].streaming = false;
            }
            self.refresh_tab_conversations(index)?;
        }
        Ok(())
    }

    pub(crate) fn cancel_active_request(&mut self) -> Result<()> {
        let Some(id) = self.active_request_id() else {
            self.ui
                .show_toast("This conversation has no active request.".into());
            return Ok(());
        };
        self.cancel_request(id)?;
        self.ui
            .show_toast("Request cancelled. The partial response was saved.".into());
        Ok(())
    }

    pub(crate) fn cancel_request(&mut self, id: SubmissionId) -> Result<()> {
        if let Some(request) = self.requests.get_mut(&id) {
            if let Some(handle) = request.abort.take() {
                handle.abort();
            }
            if !request.cancelled && !request.finalized {
                request.cancelled = true;
                let message = &mut request.messages[request.assistant_idx];
                if !message.content.is_empty() {
                    message.content.push_str("\n\n");
                }
                message.content.push_str("[Response cancelled]");
            }
        }
        for index in 0..self.ui.tabs.len() {
            self.sync_request_view(index);
        }
        self.ensure_request_destination(id)?;
        self.finish_request(id)
    }
}

#[cfg(test)]
#[path = "requests_tests.rs"]
mod tests;
