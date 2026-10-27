use std::collections::BTreeMap;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::{
    layout::{Position, Rect},
    style::{Modifier, Style},
    widgets::{Block, Borders, Padding, Paragraph, Wrap},
    Frame,
};

use super::SettingsIntent;
use crate::tui::{
    keybind_capture::{CaptureResult, KeybindCaptureState},
    palette::{Command, CommandCategory},
    settings_panel::{all_settings, command_action, SettingType},
};

/// Compose keyboard actions from the existing command registries, without another ID catalog.
pub(super) fn commands() -> Vec<Command> {
    let mut commands = crate::tui::palette::all_commands();
    for setting in all_settings() {
        let SettingType::Command(id) = setting.setting_type else {
            continue;
        };
        if commands.iter().any(|command| command.id == id) {
            continue;
        }
        if let Some(action) = command_action(id) {
            commands.push(Command {
                id: id.into(),
                title: setting.title.into(),
                description: setting.description.into(),
                category: CommandCategory::System,
                keywords: Vec::new(),
                shortcut: None,
                action,
                curated: false,
            });
        }
    }
    commands
}

fn normalized(value: &str) -> String {
    value.to_ascii_lowercase().replace(' ', "")
}

pub(super) fn canonical(value: &str) -> Result<String, String> {
    let value = normalized(value);
    let parts: Vec<_> = value.split('+').collect();
    let key = parts.last().copied().unwrap_or("");
    let modifiers = &parts[..parts.len().saturating_sub(1)];
    let function = key
        .strip_prefix('f')
        .and_then(|value| value.parse::<u8>().ok())
        .is_some_and(|value| (1..=12).contains(&value));
    if (!function && (key.chars().count() != 1 || !key.chars().all(|c| c.is_ascii_alphanumeric())))
        || modifiers
            .iter()
            .any(|modifier| !["ctrl", "alt", "shift"].contains(modifier))
        || (!function
            && !modifiers
                .iter()
                .any(|modifier| ["ctrl", "alt"].contains(modifier)))
        || modifiers
            .iter()
            .enumerate()
            .any(|(index, value)| modifiers[..index].contains(value))
    {
        return Err(
            "Use a Ctrl/Alt chord or F1–F12. Plain typing and navigation keys are reserved.".into(),
        );
    }
    if [
        "ctrl+p", "ctrl+c", "ctrl+v", "ctrl+x", "ctrl+a", "ctrl+e", "ctrl+u", "ctrl+w", "ctrl+h",
        "ctrl+j", "ctrl+m", "ctrl+i", "ctrl+z", "ctrl+t", "ctrl+f", "ctrl+l", "ctrl+q", "f6", "f7",
        "alt+1", "alt+2", "alt+3", "alt+4", "alt+5", "alt+6", "alt+7", "alt+8", "alt+9",
    ]
    .contains(&value.as_str())
    {
        return Err(format!("Reserved shortcut: {value}"));
    }
    let mut canonical = Vec::new();
    for modifier in ["ctrl", "alt", "shift"] {
        if modifiers.contains(&modifier) {
            canonical.push(modifier);
        }
    }
    canonical.push(key);
    Ok(canonical.join("+"))
}

pub(super) fn validate(bindings: &BTreeMap<String, String>) -> Result<(), String> {
    let commands = commands();
    let mut effective = BTreeMap::new();
    for command in &commands {
        if let Some(shortcut) = &command.shortcut {
            effective.insert(command.id.as_str(), normalized(shortcut));
        }
    }
    for (id, binding) in bindings {
        if !commands.iter().any(|command| command.id == *id) {
            return Err(format!("Unknown keyboard action: {id}"));
        }
        effective.insert(id, canonical(binding)?);
    }
    let mut used = BTreeMap::new();
    for (id, binding) in effective {
        if let Some(other) = used.insert(binding.clone(), id) {
            return Err(format!(
                "Shortcut {binding} conflicts between {other} and {id}"
            ));
        }
    }
    Ok(())
}

#[derive(Debug, Clone)]
pub(super) struct KeyboardSettings {
    commands: Vec<Command>,
    query: String,
    searching: bool,
    capture: Option<KeybindCaptureState>,
    pending: Option<SettingsIntent>,
    reset_areas: Vec<(Rect, usize)>,
    pub feedback: String,
    pub has_error: bool,
}

impl Default for KeyboardSettings {
    fn default() -> Self {
        Self { commands: commands(), query: String::new(), searching: false, capture: None, pending: None, reset_areas: Vec::new(),
            feedback: "Enter captures a shortcut · Backspace resets the selected action · / searches actions".into(), has_error: false }
    }
}

impl KeyboardSettings {
    fn results(&self) -> impl Iterator<Item = &Command> {
        self.commands.iter().filter(|command| {
            self.query.is_empty()
                || command.title.to_lowercase().contains(&self.query)
                || command.id.contains(&self.query)
        })
    }

    pub fn count(&self) -> usize {
        self.results().count() + 1
    }
    pub fn is_capturing(&self) -> bool {
        self.capture.is_some()
    }

    pub fn pending_draft(&self) -> Option<(String, String)> {
        match &self.pending {
            Some(SettingsIntent::Apply { id, value }) => Some((id.clone(), value.clone())),
            _ => None,
        }
    }

    pub fn select_action(&mut self, id: &str) -> Option<usize> {
        self.query.clear();
        self.searching = false;
        self.commands.iter().position(|command| command.id == id)
    }

    pub fn activate(&mut self, selected: usize) -> bool {
        let command = self
            .results()
            .nth(selected)
            .map(|command| (command.id.clone(), command.title.clone()));
        if let Some((id, title)) = command {
            self.capture = Some(KeybindCaptureState::new(id, title));
            self.pending = None;
            self.feedback =
                "Press the new shortcut. Esc cancels; Backspace restores the default.".into();
            self.has_error = false;
            false
        } else {
            true
        }
    }

    fn intent(&mut self, bindings: BTreeMap<String, String>) -> Option<SettingsIntent> {
        self.pending = None;
        match validate(&bindings) {
            Ok(()) => match serde_json::to_string(&bindings) {
                Ok(value) => {
                    let intent = SettingsIntent::Apply {
                        id: "tui.keybinding_overrides".into(),
                        value,
                    };
                    self.pending = Some(intent.clone());
                    Some(intent)
                }
                Err(error) => {
                    self.feedback = error.to_string();
                    self.has_error = true;
                    None
                }
            },
            Err(error) => {
                self.feedback = error;
                self.has_error = true;
                None
            }
        }
    }

    pub fn applied(&mut self) {
        self.capture = None;
        self.has_error = false;
        self.pending = None;
        self.feedback = "Shortcut applied and saved.".into();
    }

    pub fn fail(&mut self, message: String) {
        self.feedback = message;
        self.has_error = true;
    }

    /// Some means the keyboard editor consumed the key, even without an apply intent.
    pub fn handle_key(
        &mut self,
        key: KeyEvent,
        selected: &mut usize,
        bindings: &BTreeMap<String, String>,
    ) -> Option<Option<SettingsIntent>> {
        if self.has_error && self.pending.is_some() {
            if key.code == KeyCode::Enter
                || (key.code == KeyCode::Char('s') && key.modifiers == KeyModifiers::CONTROL)
            {
                return Some(self.pending.clone());
            }
            if key.code == KeyCode::Esc {
                self.pending = None;
                self.capture = None;
                self.has_error = false;
                self.feedback = "Shortcut change cancelled; saved binding unchanged.".into();
                return Some(None);
            }
        }
        if let Some(capture) = &mut self.capture {
            let id = capture.action_id.clone();
            let mut effective = BTreeMap::new();
            for command in &self.commands {
                if let Some(binding) = bindings.get(&command.id).or(command.shortcut.as_ref()) {
                    effective.insert(command.id.clone(), normalized(binding));
                }
            }
            match capture.capture_with_overrides(key, &effective) {
                CaptureResult::Cancelled => {
                    self.capture = None;
                    self.feedback = "Shortcut capture cancelled; saved binding unchanged.".into();
                    self.has_error = false;
                }
                CaptureResult::Cleared => {
                    let mut updated = bindings.clone();
                    updated.remove(&id);
                    return Some(self.intent(updated));
                }
                CaptureResult::Captured(_) => {
                    let Some(binding) = capture.confirm() else {
                        return Some(None);
                    };
                    let mut updated = bindings.clone();
                    updated.insert(id, binding);
                    return Some(self.intent(updated));
                }
                CaptureResult::Conflict(message) => {
                    self.feedback = message;
                    self.has_error = true;
                }
                CaptureResult::Waiting => {}
            }
            return Some(None);
        }
        if self.searching {
            match key.code {
                KeyCode::Esc | KeyCode::Enter => self.searching = false,
                KeyCode::Backspace => {
                    self.query.pop();
                    *selected = 0;
                }
                KeyCode::Char(character)
                    if !key
                        .modifiers
                        .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT) =>
                {
                    self.query.extend(character.to_lowercase());
                    *selected = 0;
                }
                _ => {}
            }
            return Some(None);
        }
        match key.code {
            KeyCode::Char('/') => {
                self.searching = true;
                *selected = 0;
                Some(None)
            }
            KeyCode::Backspace | KeyCode::Delete => {
                let id = self
                    .results()
                    .nth(*selected)
                    .map(|command| command.id.clone());
                if let Some(id) = id {
                    let mut updated = bindings.clone();
                    updated.remove(&id);
                    Some(self.intent(updated))
                } else {
                    Some(None)
                }
            }
            _ => None,
        }
    }

    pub fn reset_at(
        &mut self,
        position: Position,
        bindings: &BTreeMap<String, String>,
    ) -> Option<Option<SettingsIntent>> {
        let selected = self
            .reset_areas
            .iter()
            .find(|(rect, _)| rect.contains(position))
            .map(|(_, selected)| *selected)?;
        let id = self
            .results()
            .nth(selected)
            .map(|command| command.id.clone())?;
        let mut updated = bindings.clone();
        updated.remove(&id);
        Some(self.intent(updated))
    }

    pub fn render(
        &mut self,
        frame: &mut Frame,
        [list, description, feedback, help]: [Rect; 4],
        selected: usize,
        bindings: &BTreeMap<String, String>,
        focused: bool,
        hit_areas: &mut Vec<(Rect, usize)>,
    ) {
        let theme = crate::theme::active_theme();
        let base = Style::default().fg(theme.foreground).bg(theme.background);
        let block = Block::default().borders(Borders::ALL).title(format!(
            " Keyboard · / {}{} ",
            self.query,
            if self.searching { "▏" } else { "" }
        ));
        let inner = block.inner(list);
        frame.render_widget(block, list);
        let count = self.results().count();
        let offset = selected.saturating_sub(inner.height.saturating_sub(1) as usize);
        let mut reset_areas = std::mem::take(&mut self.reset_areas);
        reset_areas.clear();
        for (index, command) in self
            .results()
            .enumerate()
            .skip(offset)
            .take(inner.height as usize)
        {
            let current = bindings
                .get(&command.id)
                .map(String::as_str)
                .or(command.shortcut.as_deref())
                .unwrap_or("unbound");
            let style = if index == selected && focused {
                base.fg(theme.accent).add_modifier(Modifier::REVERSED)
            } else {
                base
            };
            let rect = Rect::new(inner.x, inner.y + (index - offset) as u16, inner.width, 1);
            frame.render_widget(
                Paragraph::new(format!(" {}  {}", command.title, current))
                    .block(
                        Block::default().padding(Padding::right(if rect.width >= 18 {
                            9
                        } else {
                            0
                        })),
                    )
                    .style(style),
                rect,
            );
            if rect.width >= 18 {
                let reset = Rect::new(rect.right() - 8, rect.y, 8, 1);
                frame.render_widget(
                    Paragraph::new("[Reset]").style(base.fg(theme.accent)),
                    reset,
                );
                reset_areas.push((reset, index));
            }
            hit_areas.push((rect, index));
        }
        if count >= offset && count < offset + inner.height as usize {
            let rect = Rect::new(inner.x, inner.y + (count - offset) as u16, inner.width, 1);
            let style = if selected == count && focused {
                base.fg(theme.accent).add_modifier(Modifier::REVERSED)
            } else {
                base
            };
            frame.render_widget(
                Paragraph::new(" Bulk mappings: command=chord").style(style),
                rect,
            );
            hit_areas.push((rect, count));
        }
        self.reset_areas = reset_areas;
        let copy = if let Some(capture) = &self.capture {
            format!("Capturing: {}\nPress a Ctrl/Alt chord or function key. Esc cancels. Backspace restores default. Reserved shortcuts and conflicts are rejected.", capture.action_label)
        } else if let Some(command) = self.results().nth(selected) {
            format!(
                "{} · {}\n{}\nCurrent: {} · Default: {}",
                command.title,
                command.id,
                command.description,
                bindings
                    .get(&command.id)
                    .map(String::as_str)
                    .or(command.shortcut.as_deref())
                    .unwrap_or("unbound"),
                command.shortcut.as_deref().unwrap_or("unbound")
            )
        } else {
            "Advanced bulk editor. Each action can also be captured or reset independently above."
                .into()
        };
        frame.render_widget(
            Paragraph::new(copy).wrap(Wrap { trim: true }).style(base),
            description,
        );
        frame.render_widget(
            Paragraph::new(&*self.feedback)
                .wrap(Wrap { trim: true })
                .style(base.fg(if self.has_error {
                    theme.error
                } else {
                    theme.accent
                })),
            feedback,
        );
        frame.render_widget(
            Paragraph::new(
                "/ search · Enter capture · Backspace reset · Esc cancel · Tab categories",
            )
            .style(base),
            help,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn registry_actions_validate_defaults_conflicts_and_reserved_shortcuts() {
        assert!(validate(&BTreeMap::new()).is_ok());
        assert!(validate(&BTreeMap::from([("new_chat".into(), "alt+n".into())])).is_ok());
        assert!(validate(&BTreeMap::from([("new_chat".into(), "ctrl+p".into())])).is_err());
        assert!(validate(&BTreeMap::from([("new_chat".into(), "ctrl+r".into())])).is_err());
    }
    #[test]
    fn capture_cancel_and_reset_never_require_json_entry() {
        let mut editor = KeyboardSettings::default();
        let mut selected = editor.select_action("new_chat").expect("command");
        editor.activate(selected);
        let result = editor.handle_key(
            KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE),
            &mut selected,
            &BTreeMap::new(),
        );
        assert_eq!(result, Some(None));
        assert!(!editor.is_capturing());
        editor.activate(selected);
        let result = editor.handle_key(
            KeyEvent::new(KeyCode::Char('n'), KeyModifiers::ALT),
            &mut selected,
            &BTreeMap::new(),
        );
        assert!(matches!(result, Some(Some(SettingsIntent::Apply { .. }))));
    }
    #[test]
    fn failed_persistence_retains_the_captured_intent_for_retry_or_cancel() {
        let mut editor = KeyboardSettings::default();
        let mut selected = editor.select_action("new_chat").expect("command");
        editor.activate(selected);
        let proposed = editor.handle_key(
            KeyEvent::new(KeyCode::Char('n'), KeyModifiers::ALT),
            &mut selected,
            &BTreeMap::new(),
        );
        editor.fail("disk full".into());
        assert_eq!(
            editor.handle_key(
                KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE),
                &mut selected,
                &BTreeMap::new()
            ),
            proposed
        );
        assert_eq!(
            editor.handle_key(
                KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE),
                &mut selected,
                &BTreeMap::new()
            ),
            Some(None)
        );
        assert!(!editor.is_capturing());
    }

    #[test]
    fn action_search_and_mouse_reset_use_registry_metadata() {
        let mut editor = KeyboardSettings::default();
        let mut selected = 0;
        editor.handle_key(
            KeyEvent::new(KeyCode::Char('/'), KeyModifiers::NONE),
            &mut selected,
            &BTreeMap::new(),
        );
        for character in "new chat".chars() {
            editor.handle_key(
                KeyEvent::new(KeyCode::Char(character), KeyModifiers::NONE),
                &mut selected,
                &BTreeMap::new(),
            );
        }
        assert_eq!(editor.results().next().expect("match").id, "new_chat");
        let bindings = BTreeMap::from([("new_chat".into(), "alt+n".into())]);
        let mut terminal =
            ratatui::Terminal::new(ratatui::backend::TestBackend::new(90, 20)).expect("terminal");
        let mut hits = Vec::new();
        terminal
            .draw(|frame| {
                editor.render(
                    frame,
                    [
                        Rect::new(0, 0, 90, 12),
                        Rect::new(0, 12, 90, 4),
                        Rect::new(0, 16, 90, 3),
                        Rect::new(0, 19, 90, 1),
                    ],
                    0,
                    &bindings,
                    true,
                    &mut hits,
                )
            })
            .expect("render");
        let rendered = terminal.backend().to_string();
        assert!(rendered.contains("alt+n"));
        assert!(rendered.contains("Ctrl+N"));
        let reset = editor.reset_areas.first().expect("mouse reset").0;
        assert_eq!(
            editor.reset_at(Position::new(reset.x, reset.y), &bindings),
            Some(Some(SettingsIntent::Apply {
                id: "tui.keybinding_overrides".into(),
                value: "{}".into()
            }))
        );
    }
}
