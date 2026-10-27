use ratatui::layout::Rect;

use super::TuiApp;

impl TuiApp {
    pub(crate) fn current_input_anchor(&self) -> Option<Rect> {
        self.ui
            .tabs
            .get(self.ui.active_tab)
            .and_then(|tab| tab.input_area)
    }

    pub(crate) fn open_chat_draft_editor(&mut self) {
        let content = self
            .ui
            .tabs
            .get(self.ui.active_tab)
            .map(|tab| tab.input_content.as_str())
            .unwrap_or_default();
        let path = chat_draft_path();
        if let Err(error) = std::fs::write(&path, content) {
            self.ui
                .show_toast(format!("Could not prepare chat draft editor: {error}"));
            return;
        }
        match crate::ui::modals::editor_popup::EditorPopupState::new_chat_draft(&path) {
            Ok(editor) => self.ui.editor_popup = Some(editor),
            Err(error) => {
                let _ = std::fs::remove_file(&path);
                self.ui.show_toast(error);
            }
        }
    }

    pub(crate) fn apply_chat_draft_from_path(&mut self, path: &std::path::Path) {
        match std::fs::read_to_string(path) {
            Ok(content) => self.replace_input_content(content),
            Err(error) => self
                .ui
                .show_toast(format!("Could not read edited chat draft: {error}")),
        }
        let _ = std::fs::remove_file(path);
    }

    pub(crate) fn take_input_submission(&mut self) -> Option<String> {
        let tab = self.ui.tabs.get_mut(self.ui.active_tab)?;
        if tab.input_content.is_empty() {
            return None;
        }
        let content = std::mem::take(&mut tab.input_content);
        tab.input_history_index = None;
        tab.input_history_draft = None;
        tab.input_cursor = 0;
        tab.input_scroll = 0;
        Some(content)
    }

    pub(crate) fn insert_input_newline(&mut self) {
        self.insert_input_char('\n');
    }

    pub(crate) fn insert_input_text(&mut self, text: &str) {
        if let Some(tab) = self.ui.tabs.get_mut(self.ui.active_tab) {
            let cursor = tab.input_cursor.min(tab.input_content.chars().count());
            let byte = char_to_byte_index(&tab.input_content, cursor);
            let needs_space = cursor > 0
                && !tab.input_content[..byte].ends_with(char::is_whitespace)
                && !text.starts_with(char::is_whitespace);
            if needs_space {
                tab.input_content.insert(byte, ' ');
                tab.input_cursor += 1;
            }
            let insert_at = char_to_byte_index(&tab.input_content, tab.input_cursor);
            tab.input_content.insert_str(insert_at, text);
            tab.input_cursor += text.chars().count();
        }
        self.refresh_input_popup();
    }

    pub(crate) fn insert_input_char(&mut self, character: char) {
        if let Some(tab) = self.ui.tabs.get_mut(self.ui.active_tab) {
            tab.input_history_index = None;
            let cursor = tab.input_cursor.min(tab.input_content.chars().count());
            let byte = char_to_byte_index(&tab.input_content, cursor);
            tab.input_content.insert(byte, character);
            tab.input_cursor = cursor + 1;
        }
        self.refresh_input_popup();
    }

    pub(crate) fn backspace_input_char(&mut self) {
        if let Some(tab) = self.ui.tabs.get_mut(self.ui.active_tab) {
            tab.input_history_index = None;
            let cursor = tab.input_cursor.min(tab.input_content.chars().count());
            if cursor == 0 {
                return;
            }
            let start = char_to_byte_index(&tab.input_content, cursor - 1);
            let end = char_to_byte_index(&tab.input_content, cursor);
            tab.input_content.replace_range(start..end, "");
            tab.input_cursor = cursor - 1;
        }
        self.refresh_input_popup();
    }

    pub(crate) fn delete_input_char(&mut self) {
        if let Some(tab) = self.ui.tabs.get_mut(self.ui.active_tab) {
            tab.input_history_index = None;
            let cursor = tab.input_cursor.min(tab.input_content.chars().count());
            if cursor >= tab.input_content.chars().count() {
                return;
            }
            let start = char_to_byte_index(&tab.input_content, cursor);
            let end = char_to_byte_index(&tab.input_content, cursor + 1);
            tab.input_content.replace_range(start..end, "");
        }
        self.refresh_input_popup();
    }

    pub(crate) fn move_input_cursor_left(&mut self) {
        if let Some(tab) = self.ui.tabs.get_mut(self.ui.active_tab) {
            tab.input_cursor = tab.input_cursor.saturating_sub(1);
        }
        self.refresh_input_popup();
    }

    pub(crate) fn move_input_cursor_right(&mut self) {
        if let Some(tab) = self.ui.tabs.get_mut(self.ui.active_tab) {
            let len = tab.input_content.chars().count();
            tab.input_cursor = (tab.input_cursor + 1).min(len);
        }
        self.refresh_input_popup();
    }

    pub(crate) fn move_input_cursor_home(&mut self) {
        if let Some(tab) = self.ui.tabs.get_mut(self.ui.active_tab) {
            tab.input_cursor = 0;
        }
        self.refresh_input_popup();
    }

    pub(crate) fn move_input_cursor_end(&mut self) {
        if let Some(tab) = self.ui.tabs.get_mut(self.ui.active_tab) {
            tab.input_cursor = tab.input_content.chars().count();
        }
        self.refresh_input_popup();
    }

    pub(crate) fn set_input_cursor_from_click(&mut self, position: ratatui::layout::Position) {
        let Some(tab) = self.ui.tabs.get_mut(self.ui.active_tab) else {
            return;
        };
        let Some(area) = tab.input_text_area else {
            return;
        };
        if !area.contains(position) {
            return;
        }
        let layout = crate::ui::chat_tab::input_layout(
            &tab.input_content,
            tab.input_cursor,
            tab.input_scroll,
            area.width as usize,
            area.height as usize,
            true,
        );
        let relative_x = position.x.saturating_sub(area.x).saturating_sub(1) as usize;
        let relative_y = position.y.saturating_sub(area.y) as usize;
        let line_index =
            (layout.scroll + relative_y).min(layout.line_ranges.len().saturating_sub(1));
        let (start, end) = layout
            .line_ranges
            .get(line_index)
            .copied()
            .unwrap_or((0, 0));
        tab.input_cursor = (start + relative_x).min(end);
        self.refresh_input_popup();
    }

    pub(crate) fn replace_input_content(&mut self, content: String) {
        if let Some(tab) = self.ui.tabs.get_mut(self.ui.active_tab) {
            tab.input_content = content;
            tab.input_cursor = tab.input_content.chars().count();
            tab.input_scroll = 0;
            tab.input_history_index = None;
            tab.input_history_draft = None;
        }
        self.refresh_input_popup();
    }

    pub(crate) fn browse_input_history(&mut self, forward: bool) -> bool {
        let Some(tab) = self.ui.tabs.get_mut(self.ui.active_tab) else {
            return false;
        };
        let history: Vec<String> = tab
            .messages
            .iter()
            .filter(|message| message.role == "user")
            .map(|message| message.content.clone())
            .collect();
        if history.is_empty() {
            return false;
        }

        if tab.input_history_index.is_none() {
            tab.input_history_draft = Some(tab.input_content.clone());
        }

        let next_index = match (tab.input_history_index, forward) {
            (None, false) => Some(history.len().saturating_sub(1)),
            (None, true) => None,
            (Some(index), false) => Some(index.saturating_sub(1)),
            (Some(index), true) if index + 1 < history.len() => Some(index + 1),
            (Some(_), true) => None,
        };

        match next_index {
            Some(index) => {
                tab.input_history_index = Some(index);
                tab.input_content = history[index].clone();
            }
            None => {
                tab.input_history_index = None;
                tab.input_content = tab.input_history_draft.clone().unwrap_or_default();
            }
        }
        tab.input_cursor = tab.input_content.chars().count();
        tab.input_scroll = 0;
        self.refresh_input_popup();
        true
    }
}

pub(crate) fn char_to_byte_index(text: &str, char_idx: usize) -> usize {
    if char_idx == 0 {
        return 0;
    }
    text.char_indices()
        .nth(char_idx)
        .map(|(idx, _)| idx)
        .unwrap_or(text.len())
}

fn chat_draft_path() -> std::path::PathBuf {
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or_default();
    std::env::temp_dir().join(format!("tcui-chat-draft-{}-{stamp}.md", std::process::id()))
}

#[cfg(test)]
mod tests {
    use super::char_to_byte_index;

    #[test]
    fn char_to_byte_index_at_start_is_zero() {
        assert_eq!(char_to_byte_index("hello", 0), 0);
    }

    #[test]
    fn char_to_byte_index_counts_ascii_chars() {
        assert_eq!(char_to_byte_index("hello", 3), 3);
    }

    #[test]
    fn char_to_byte_index_counts_multibyte_chars() {
        let text = "héllo"; // é is two bytes
        assert_eq!(char_to_byte_index(text, 1), 1); // before é
        assert_eq!(char_to_byte_index(text, 2), 3); // after é
        assert_eq!(char_to_byte_index(text, 5), text.len());
    }

    #[test]
    fn char_to_byte_index_past_end_returns_len() {
        assert_eq!(char_to_byte_index("hi", 10), 2);
    }
}
