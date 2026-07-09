use std::collections::BTreeMap;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::{prelude::*, widgets::*};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KeybindCaptureState {
    pub action_id: String,
    pub action_label: String,
    pub captured: Option<String>,
    pub conflicting: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CaptureResult {
    Captured(String),
    Conflict(String),
    Waiting,
    Cancelled,
    Cleared,
}

impl KeybindCaptureState {
    pub fn new(action_id: impl Into<String>, action_label: impl Into<String>) -> Self {
        Self {
            action_id: action_id.into(),
            action_label: action_label.into(),
            captured: None,
            conflicting: None,
        }
    }

    pub fn capture(&mut self, key: KeyEvent) -> CaptureResult {
        if key.kind != crossterm::event::KeyEventKind::Press {
            return CaptureResult::Waiting;
        }
        match key.code {
            KeyCode::Esc => {
                self.cancel();
                CaptureResult::Cancelled
            }
            KeyCode::Backspace => {
                self.clear();
                CaptureResult::Cleared
            }
            _ => {
                let repr = key_repr(key);
                self.captured = Some(repr.clone());
                self.conflicting = None;
                CaptureResult::Captured(repr)
            }
        }
    }

    pub fn capture_with_overrides(
        &mut self,
        key: KeyEvent,
        overrides: &BTreeMap<String, String>,
    ) -> CaptureResult {
        let result = self.capture(key);
        let CaptureResult::Captured(repr) = result else {
            return result;
        };
        if let Some((action, _)) = overrides.iter().find(|(action, binding)| {
            action.as_str() != self.action_id && binding.eq_ignore_ascii_case(&repr)
        }) {
            let message = format!("Already bound to {action}");
            self.conflicting = Some(message.clone());
            return CaptureResult::Conflict(message);
        }
        CaptureResult::Captured(repr)
    }

    pub fn confirm(&self) -> Option<String> {
        self.captured.clone()
    }

    pub fn cancel(&mut self) {
        self.captured = None;
        self.conflicting = None;
    }

    pub fn clear(&mut self) {
        self.captured = None;
        self.conflicting = None;
    }

    pub fn render(&self, f: &mut Frame, area: Rect) {
        let popup = centered_rect(54, 26, area);
        f.render_widget(Clear, popup);
        f.render_widget(
            Block::default()
                .title(" Capture Key ")
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::Yellow)),
            popup,
        );

        let inner = popup.inner(Margin {
            vertical: 1,
            horizontal: 2,
        });
        let status = self
            .conflicting
            .as_deref()
            .or(self.captured.as_deref())
            .unwrap_or("Waiting for input...");
        let status_style = if self.conflicting.is_some() {
            Style::default().fg(Color::Red)
        } else {
            Style::default().fg(Color::Green)
        };
        f.render_widget(
            Paragraph::new(vec![
                Line::from(format!("Press key to bind to {}", self.action_label)),
                Line::from("Esc to cancel"),
                Line::from("Backspace to clear"),
                Line::from(""),
                Line::styled(status, status_style),
            ])
            .wrap(Wrap { trim: true }),
            inner,
        );
    }
}

pub fn key_repr(key: KeyEvent) -> String {
    let mut parts = Vec::new();
    if key.modifiers.contains(KeyModifiers::CONTROL) {
        parts.push("ctrl".to_string());
    }
    if key.modifiers.contains(KeyModifiers::ALT) {
        parts.push("alt".to_string());
    }
    if key.modifiers.contains(KeyModifiers::SHIFT) && !matches!(key.code, KeyCode::Char(_)) {
        parts.push("shift".to_string());
    }
    parts.push(match key.code {
        KeyCode::Backspace => "backspace".to_string(),
        KeyCode::Enter => "enter".to_string(),
        KeyCode::Left => "left".to_string(),
        KeyCode::Right => "right".to_string(),
        KeyCode::Up => "up".to_string(),
        KeyCode::Down => "down".to_string(),
        KeyCode::Home => "home".to_string(),
        KeyCode::End => "end".to_string(),
        KeyCode::PageUp => "pageup".to_string(),
        KeyCode::PageDown => "pagedown".to_string(),
        KeyCode::Tab => "tab".to_string(),
        KeyCode::BackTab => "shift+tab".to_string(),
        KeyCode::Delete => "delete".to_string(),
        KeyCode::Insert => "insert".to_string(),
        KeyCode::F(n) => format!("f{n}"),
        // Crossterm can only report Ctrl+key combinations that the terminal
        // passes through; some terminals intercept Ctrl+. and similar chords
        // before applications can see them.
        KeyCode::Char('.') if key.modifiers.contains(KeyModifiers::CONTROL) => ".".to_string(),
        KeyCode::Char(c) => c.to_lowercase().to_string(),
        KeyCode::Null => "null".to_string(),
        KeyCode::Esc => "esc".to_string(),
        KeyCode::CapsLock => "capslock".to_string(),
        KeyCode::ScrollLock => "scrolllock".to_string(),
        KeyCode::NumLock => "numlock".to_string(),
        KeyCode::PrintScreen => "printscreen".to_string(),
        KeyCode::Pause => "pause".to_string(),
        KeyCode::Menu => "menu".to_string(),
        KeyCode::KeypadBegin => "keypadbegin".to_string(),
        KeyCode::Media(_) => "media".to_string(),
        KeyCode::Modifier(_) => "modifier".to_string(),
    });
    parts.join("+")
}

fn centered_rect(percent_x: u16, percent_y: u16, area: Rect) -> Rect {
    let vertical = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage((100 - percent_y) / 2),
            Constraint::Percentage(percent_y),
            Constraint::Percentage((100 - percent_y) / 2),
        ])
        .split(area);
    Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage((100 - percent_x) / 2),
            Constraint::Percentage(percent_x),
            Constraint::Percentage((100 - percent_x) / 2),
        ])
        .split(vertical[1])[1]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crossterm::event::{KeyEventKind, KeyEventState};

    fn key(code: KeyCode, modifiers: KeyModifiers) -> KeyEvent {
        KeyEvent {
            code,
            modifiers,
            kind: KeyEventKind::Press,
            state: KeyEventState::NONE,
        }
    }

    #[test]
    fn capture_esc_returns_cancelled_when_waiting() {
        let mut state = KeybindCaptureState::new("open_palette", "Open Palette");

        let result = state.capture(key(KeyCode::Esc, KeyModifiers::NONE));

        assert_eq!(result, CaptureResult::Cancelled);
        assert_eq!(state.confirm(), None);
    }

    #[test]
    fn capture_backspace_returns_cleared_when_waiting() {
        let mut state = KeybindCaptureState::new("open_palette", "Open Palette");

        let result = state.capture(key(KeyCode::Backspace, KeyModifiers::NONE));

        assert_eq!(result, CaptureResult::Cleared);
        assert_eq!(state.confirm(), None);
    }

    #[test]
    fn capture_ctrl_p_returns_normalized_repr() {
        let mut state = KeybindCaptureState::new("open_palette", "Open Palette");

        let result = state.capture(key(KeyCode::Char('p'), KeyModifiers::CONTROL));

        assert_eq!(result, CaptureResult::Captured("ctrl+p".to_string()));
    }

    #[test]
    fn capture_ctrl_dot_returns_normalized_repr_when_terminal_reports_it() {
        let mut state = KeybindCaptureState::new("open_palette", "Open Palette");

        let result = state.capture(key(KeyCode::Char('.'), KeyModifiers::CONTROL));

        assert_eq!(result, CaptureResult::Captured("ctrl+.".to_string()));
    }

    #[test]
    fn conflict_detection_sets_conflicting_when_other_action_uses_binding() {
        let mut state = KeybindCaptureState::new("open_palette", "Open Palette");
        let overrides = BTreeMap::from([("new_chat".to_string(), "ctrl+p".to_string())]);

        let result = state
            .capture_with_overrides(key(KeyCode::Char('p'), KeyModifiers::CONTROL), &overrides);

        assert_eq!(
            result,
            CaptureResult::Conflict("Already bound to new_chat".to_string())
        );
        assert_eq!(
            state.conflicting.as_deref(),
            Some("Already bound to new_chat")
        );
    }

    #[test]
    fn confirm_returns_captured_key_repr() {
        let mut state = KeybindCaptureState::new("open_palette", "Open Palette");
        let _ = state.capture(key(KeyCode::F(1), KeyModifiers::NONE));

        assert_eq!(state.confirm().as_deref(), Some("f1"));
    }

    #[test]
    fn clear_produces_none_binding() {
        let mut state = KeybindCaptureState::new("open_palette", "Open Palette");
        let _ = state.capture(key(KeyCode::Char('p'), KeyModifiers::CONTROL));

        state.clear();

        assert_eq!(state.confirm(), None);
    }
}
