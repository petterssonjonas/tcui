use super::{Action, TuiApp};
use crate::config::tui::TerminalPlacement;
use crate::ui::app_tabs::AppView;
use crate::ui::command_review::{CommandReview, ReviewControl};
use crate::ui::terminal_workspace::{TerminalControl, TerminalSession};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use ratatui::layout::{Position, Rect};
use std::path::PathBuf;

impl TuiApp {
    fn terminal_start_directory(&self) -> Result<PathBuf, String> {
        let configured = self
            .config
            .try_read()
            .map_err(|_| "Configuration is busy".to_string())?
            .tui
            .terminal_start_directory
            .clone();
        let directory = match configured.filter(|value| !value.is_empty()) {
            Some(value) => PathBuf::from(value),
            None => std::env::current_dir().map_err(|error| error.to_string())?,
        };
        std::fs::canonicalize(directory).map_err(|error| format!("Terminal directory: {error}"))
    }

    fn terminal_host() -> String {
        std::fs::read_to_string("/proc/sys/kernel/hostname")
            .ok()
            .map(|value| value.trim().to_owned())
            .filter(|value| !value.is_empty())
            .unwrap_or_else(|| "TCUI execution host".to_string())
    }

    async fn create_terminal(
        &mut self,
        directory: PathBuf,
        shell_override: Option<&str>,
    ) -> Result<u64, String> {
        let (shell, placement) = {
            let config = self
                .config
                .try_read()
                .map_err(|_| "Configuration is busy".to_string())?;
            (
                config.tui.terminal_shell.clone(),
                config.tui.terminal_placement,
            )
        };
        let area = self.ui.last_area.unwrap_or(Rect::new(0, 0, 80, 24));
        let terminal = match shell_override {
            Some(shell) => {
                crate::ui::modals::terminal_editor::TerminalEditorState::new_command_shell(
                    shell, &directory, area,
                )
                .await
            }
            None => {
                crate::ui::modals::terminal_editor::TerminalEditorState::new_shell(
                    shell.as_deref(),
                    &directory,
                    area,
                )
                .await
            }
        }
        .map_err(|error| error.to_string())?;
        let id = self.ui.terminals.insert(TerminalSession {
            terminal,
            cwd: directory,
            host: Self::terminal_host(),
        });
        self.ui
            .app_tabs
            .open_view(AppView::Terminal { session_id: id });
        self.show_terminal(id, placement);
        Ok(id)
    }

    pub(crate) async fn open_terminal(&mut self) {
        let result = match self.terminal_start_directory() {
            Ok(directory) => self.create_terminal(directory, None).await,
            Err(error) => Err(error),
        };
        if let Err(error) = result {
            self.ui.show_toast(error);
        }
    }

    fn show_terminal(&mut self, id: u64, placement: TerminalPlacement) {
        if !self.ui.terminals.sessions.contains_key(&id) {
            return;
        }
        self.ui.terminals.placement = placement;
        self.ui.terminals.focused = true;
        match placement {
            TerminalPlacement::Tab => {
                if self.ui.terminals.split_session == Some(id) {
                    self.ui.terminals.split_session = None;
                }
                self.ui
                    .app_tabs
                    .open_view(AppView::Terminal { session_id: id });
            }
            TerminalPlacement::Bottom | TerminalPlacement::Right => {
                self.ui.terminals.split_session = Some(id);
                self.ui.app_tabs.select(0);
            }
        }
        self.ui.focus = crate::tui::focus::Focus::Chat;
    }

    pub(crate) fn reflow_terminal_placement(&mut self, placement: TerminalPlacement) {
        let active = self.ui.app_tabs.active_view();
        let id = match active {
            AppView::Terminal { session_id } => Some(session_id),
            _ => self
                .ui
                .terminals
                .split_session
                .or_else(|| self.ui.terminals.sessions.keys().next_back().copied()),
        };
        self.ui.terminals.placement = placement;
        self.ui.terminals.split_session = if matches!(placement, TerminalPlacement::Tab) {
            None
        } else {
            id
        };
        if let (AppView::Terminal { .. }, Some(id)) = (active, id) {
            self.show_terminal(id, placement);
        }
    }

    pub(crate) fn request_terminal_close(&mut self, id: u64) {
        if self
            .ui
            .terminals
            .sessions
            .get(&id)
            .is_some_and(|session| !session.terminal.is_done())
        {
            self.ui.terminals.close_pending = Some(id);
        } else {
            self.close_terminal(id);
        }
    }

    fn close_terminal(&mut self, id: u64) {
        if let Some(mut session) = self.ui.terminals.sessions.remove(&id) {
            session.terminal.confirm_close();
        }
        if self.ui.terminals.split_session == Some(id) {
            self.ui.terminals.split_session = None;
        }
        self.ui.terminals.close_pending = None;
        self.ui.terminals.focused = false;
        let tab_id = self
            .ui
            .app_tabs
            .tabs()
            .iter()
            .find(|tab| tab.view() == &(AppView::Terminal { session_id: id }))
            .map(|tab| tab.id());
        if let Some(tab_id) = tab_id {
            self.ui.app_tabs.close(tab_id);
        }
    }

    pub(crate) fn poll_terminals(&mut self) {
        let mut errors = Vec::new();
        for session in self.ui.terminals.sessions.values_mut() {
            session.terminal.poll_output();
            if let Some(error) = session.terminal.take_error() {
                errors.push(error);
            }
        }
        for error in errors {
            self.ui.show_toast(format!("Terminal: {error}"));
        }
    }

    pub(crate) fn shutdown_terminals(&mut self) {
        for session in self.ui.terminals.sessions.values_mut() {
            session.terminal.confirm_close();
        }
    }

    pub(crate) fn open_command_review(&mut self, message_index: usize) {
        let Some(message) = self
            .ui
            .tabs
            .get(self.ui.active_tab)
            .filter(|tab| !(tab.streaming && message_index.saturating_add(1) == tab.messages.len()))
            .and_then(|tab| tab.messages.get(message_index))
            .filter(|message| message.role == "assistant")
        else {
            return;
        };
        let blocks = crate::ui::command_blocks::shell_blocks(&message.content);
        if blocks.is_empty() {
            return;
        }
        match self.terminal_start_directory() {
            // The review snapshots its executable; changes elsewhere cannot alter approval.
            Ok(directory) => {
                let mut review = CommandReview::new(blocks, directory, Self::terminal_host(), None);
                if let Ok(config) = self.config.try_read() {
                    if let Some(shell) = config
                        .tui
                        .terminal_shell
                        .as_ref()
                        .filter(|value| !value.is_empty())
                    {
                        review.default_shell = shell.clone();
                    }
                }
                self.ui.command_review = Some(review);
            }
            Err(error) => self.ui.show_toast(error),
        }
    }

    fn cycle_review_destination(&mut self) {
        let Some(review) = self.ui.command_review.as_ref() else {
            return;
        };
        let next = match review.destination {
            None => self
                .ui
                .terminals
                .sessions
                .iter()
                .find(|(_, session)| !session.terminal.is_done())
                .map(|(id, _)| *id),
            Some(current) => self
                .ui
                .terminals
                .sessions
                .range((
                    std::ops::Bound::Excluded(current),
                    std::ops::Bound::Unbounded,
                ))
                .find(|(_, session)| !session.terminal.is_done())
                .map(|(id, _)| *id),
        };
        let context = match next {
            Some(id) => self.ui.terminals.sessions.get(&id).map(|session| {
                let directory = session.terminal.current_directory();
                let known = directory.is_some();
                (
                    directory.unwrap_or_else(|| session.cwd.clone()),
                    session.host.clone(),
                    session.terminal.shell_program().map(str::to_string),
                    known,
                )
            }),
            None => match self.terminal_start_directory() {
                Ok(directory) => Some((directory, Self::terminal_host(), None, true)),
                Err(error) => {
                    self.ui.show_toast(error);
                    return;
                }
            },
        };
        if let (Some(review), Some((directory, host, shell, known))) =
            (self.ui.command_review.as_mut(), context)
        {
            review.destination = next;
            review.cwd = directory;
            review.host = host;
            review.destination_shell = shell;
            review.cwd_is_current = known;
        }
    }

    fn review_control(&mut self, control: ReviewControl) -> Option<Action> {
        match control {
            ReviewControl::Cancel => self.ui.command_review = None,
            ReviewControl::Approve => {
                if self
                    .ui
                    .command_review
                    .as_ref()
                    .is_some_and(|review| review.destination.is_some())
                {
                    self.ui.show_toast(
                        "Existing destination: use Paste (no Enter), not Run".to_string(),
                    );
                    return None;
                }
                return Some(Action::ApproveCommandReview);
            }
            ReviewControl::Transfer => return Some(Action::TransferCommandReview),
            ReviewControl::Destination => self.cycle_review_destination(),
            ReviewControl::Copy => {
                if let Some(review) = self.ui.command_review.as_ref() {
                    let result = crate::clipboard::copy(&review.command());
                    self.ui.show_toast(match result {
                        Ok(()) => "Copy sent to clipboard".to_string(),
                        Err(error) => format!("Copy failed: {error}"),
                    });
                }
            }
            ReviewControl::Block(index) => {
                if let Some(review) = self.ui.command_review.as_mut() {
                    if let Some(selected) = review.selected.get_mut(index) {
                        *selected = !*selected;
                    }
                }
            }
        }
        None
    }

    pub(crate) async fn approve_command_review(&mut self) {
        // Consume approval before any fallible operation: errors never retry a command.
        let Some(review) = self.ui.command_review.take() else {
            return;
        };
        let command = review.command();
        if command.is_empty() {
            self.ui.show_toast("No command selected".to_string());
            return;
        }
        let shell = match review.shell() {
            Ok(shell) => shell,
            Err(error) => {
                self.ui.show_toast(error.to_string());
                return;
            }
        };
        if review.destination.is_some() {
            self.ui.show_toast(
                "Existing terminals accept explicit Paste only; select a new terminal to Run"
                    .to_string(),
            );
            return;
        }
        let id = match self.create_terminal(review.cwd, Some(&shell)).await {
            Ok(id) => id,
            Err(error) => {
                self.ui.show_toast(error);
                return;
            }
        };
        if let Some(session) = self.ui.terminals.sessions.get_mut(&id) {
            if let Err(error) = session.terminal.send_approved_commands(&command) {
                self.ui
                    .show_toast(format!("Command delivery failed; not retried: {error}"));
            }
        }
        let placement = self.ui.terminals.placement;
        self.show_terminal(id, placement);
    }

    pub(crate) fn transfer_command_review(&mut self) {
        let Some(review) = self.ui.command_review.take() else {
            return;
        };
        let Some(id) = review.destination else {
            self.ui
                .show_toast("Choose an existing terminal before Paste (no Enter)".to_string());
            return;
        };
        let command = review.command();
        let Some(session) = self.ui.terminals.sessions.get_mut(&id) else {
            return;
        };
        if session.terminal.is_done() {
            self.ui
                .show_toast("Terminal exited; nothing pasted".to_string());
            return;
        }
        match session.terminal.paste(&command) {
            Ok(()) => self.ui.show_toast(
                "Pasted without Enter. Inspect the foreground program and input before submitting."
                    .to_string(),
            ),
            Err(error) => self
                .ui
                .show_toast(format!("Paste rejected; nothing retried: {error}")),
        }
        let placement = self.ui.terminals.placement;
        self.show_terminal(id, placement);
    }

    pub(crate) fn terminal_key(&mut self, key: KeyEvent) -> Option<Option<Action>> {
        if let Some(id) = self.ui.terminals.close_pending {
            match key.code {
                KeyCode::Char('y' | 'Y') => self.close_terminal(id),
                KeyCode::Esc | KeyCode::Char('n' | 'N') => self.ui.terminals.close_pending = None,
                _ => {}
            }
            return Some(None);
        }
        if self.ui.command_review.is_some() {
            let control = match key.code {
                KeyCode::Esc => Some(ReviewControl::Cancel),
                KeyCode::Char('r') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                    Some(ReviewControl::Approve)
                }
                KeyCode::Tab => Some(ReviewControl::Destination),
                KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                    Some(ReviewControl::Copy)
                }
                KeyCode::Char('v') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                    Some(ReviewControl::Transfer)
                }
                KeyCode::Char(' ') => self
                    .ui
                    .command_review
                    .as_ref()
                    .map(|review| ReviewControl::Block(review.cursor)),
                _ => None,
            };
            if let Some(control) = control {
                return Some(self.review_control(control));
            }
            if let Some(review) = self.ui.command_review.as_mut() {
                match key.code {
                    KeyCode::Up => review.cursor = review.cursor.saturating_sub(1),
                    KeyCode::Down => {
                        review.cursor =
                            (review.cursor + 1).min(review.blocks.len().saturating_sub(1))
                    }
                    KeyCode::PageUp => review.scroll = review.scroll.saturating_sub(8),
                    KeyCode::PageDown => review.scroll = review.scroll.saturating_add(8),
                    KeyCode::Home => review.scroll = 0,
                    _ => {}
                }
            }
            return Some(None);
        }
        if key.code == KeyCode::Char('p') && key.modifiers.contains(KeyModifiers::CONTROL) {
            return Some(Some(Action::OpenCommandPalette));
        }
        let view = self.ui.app_tabs.active_view();
        let visible = match view {
            AppView::Terminal { session_id } => Some(session_id),
            AppView::Chat => self.ui.terminals.split_session,
            _ => None,
        };
        if key.code == KeyCode::F(6) {
            if let Some(id) = visible {
                if matches!(view, AppView::Terminal { .. }) {
                    self.show_terminal(id, TerminalPlacement::Bottom);
                    self.ui.terminals.focused = false;
                } else {
                    self.ui.terminals.focused = !self.ui.terminals.focused;
                }
                return Some(None);
            }
        }
        if key.code == KeyCode::F(7) {
            if let Some(id) = visible {
                let placement = if matches!(view, AppView::Terminal { .. }) {
                    TerminalPlacement::Bottom
                } else {
                    match self.ui.terminals.placement {
                        TerminalPlacement::Bottom => TerminalPlacement::Right,
                        TerminalPlacement::Right => TerminalPlacement::Tab,
                        TerminalPlacement::Tab => TerminalPlacement::Bottom,
                    }
                };
                self.show_terminal(id, placement);
                return Some(None);
            }
        }
        if self.ui.terminals.focused {
            if let Some(id) = visible {
                if let Some(session) = self.ui.terminals.sessions.get_mut(&id) {
                    session.terminal.handle_key(key);
                }
                return Some(None);
            }
        }
        None
    }

    pub(crate) fn terminal_control(&mut self, id: u64, control: TerminalControl) -> Option<Action> {
        match control {
            TerminalControl::Tab => self.show_terminal(id, TerminalPlacement::Tab),
            TerminalControl::Right => self.show_terminal(id, TerminalPlacement::Right),
            TerminalControl::Bottom => self.show_terminal(id, TerminalPlacement::Bottom),
            TerminalControl::Focus => {
                if matches!(self.ui.app_tabs.active_view(), AppView::Terminal { .. }) {
                    self.show_terminal(id, TerminalPlacement::Bottom);
                    self.ui.terminals.focused = false;
                } else {
                    self.ui.terminals.focused = !self.ui.terminals.focused;
                }
            }
            TerminalControl::New => return Some(Action::OpenTerminal),
            TerminalControl::Close => self.request_terminal_close(id),
        }
        None
    }

    pub(crate) fn terminal_mouse(&mut self, mouse: MouseEvent) -> Option<Option<Action>> {
        let pos = Position::new(mouse.column, mouse.row);
        if self.ui.command_review.is_some() {
            if mouse.kind == MouseEventKind::Up(MouseButton::Left) {
                let control = self.ui.command_review.as_ref().and_then(|review| {
                    review
                        .controls
                        .iter()
                        .find(|(area, _)| area.contains(pos))
                        .map(|(_, control)| *control)
                });
                return Some(control.and_then(|control| self.review_control(control)));
            }
            if matches!(
                mouse.kind,
                MouseEventKind::ScrollUp | MouseEventKind::ScrollDown
            ) {
                if let Some(review) = self.ui.command_review.as_mut() {
                    review.scroll = if mouse.kind == MouseEventKind::ScrollDown {
                        review.scroll.saturating_add(3)
                    } else {
                        review.scroll.saturating_sub(3)
                    };
                }
            }
            return Some(None);
        }
        if let Some(id) = self.ui.terminals.close_pending {
            if mouse.kind == MouseEventKind::Up(MouseButton::Left) {
                if let Some(areas) = self.ui.terminals.close_areas {
                    if areas.yes.contains(pos) {
                        self.close_terminal(id);
                    } else if areas.no.contains(pos) {
                        self.ui.terminals.close_pending = None;
                    }
                }
            }
            return Some(None);
        }
        if self.ui.terminals.dragging {
            match mouse.kind {
                MouseEventKind::Drag(MouseButton::Left) => self.ui.terminals.drag(pos),
                MouseEventKind::Up(MouseButton::Left) => self.ui.terminals.dragging = false,
                _ => {}
            }
            return Some(None);
        }
        if mouse.kind == MouseEventKind::Down(MouseButton::Left)
            && self
                .ui
                .terminals
                .divider
                .is_some_and(|area| area.contains(pos))
        {
            self.ui.terminals.dragging = true;
            self.ui.text_selection.cancel();
            return Some(None);
        }
        if let Some((_, id, control)) = self
            .ui
            .terminals
            .controls
            .iter()
            .find(|(area, _, _)| area.contains(pos))
            .copied()
        {
            if mouse.kind == MouseEventKind::Up(MouseButton::Left) {
                return Some(self.terminal_control(id, control));
            }
            return Some(None);
        }
        if let Some((id, area)) = self
            .ui
            .terminals
            .body
            .filter(|(_, area)| area.contains(pos))
        {
            self.ui.terminals.focused = true;
            if let Some(session) = self.ui.terminals.sessions.get_mut(&id) {
                if session.terminal.handle_mouse(mouse, area) {
                    return Some(None);
                }
                match mouse.kind {
                    MouseEventKind::ScrollUp => session.terminal.scroll(3),
                    MouseEventKind::ScrollDown => session.terminal.scroll(-3),
                    _ => {}
                }
            }
            return Some(None);
        }
        if mouse.kind == MouseEventKind::Down(MouseButton::Left)
            && self.ui.chat_area.is_some_and(|area| area.contains(pos))
        {
            self.ui.terminals.focused = false;
        }
        None
    }

    pub(crate) fn terminal_paste(&mut self, text: &str) -> bool {
        if self.ui.command_review.is_some() || self.ui.terminals.close_pending.is_some() {
            return true;
        }
        let visible = match self.ui.app_tabs.active_view() {
            AppView::Terminal { session_id } => Some(session_id),
            AppView::Chat if self.ui.terminals.focused => self.ui.terminals.split_session,
            _ => None,
        };
        let Some(id) = visible else {
            return false;
        };
        let Some(session) = self.ui.terminals.sessions.get_mut(&id) else {
            return true;
        };
        if text.chars().any(char::is_control) {
            if text
                .chars()
                .any(|ch| ch.is_control() && ch != '\n' && ch != '\t')
            {
                self.ui.show_toast(
                    "Terminal paste rejected: control characters can execute hidden input"
                        .to_string(),
                );
            } else {
                let mut review = CommandReview::new(
                    vec![crate::ui::command_blocks::CommandBlock {
                        language: "pasted shell input".to_string(),
                        code: text.to_string(),
                    }],
                    session.directory(),
                    session.host.clone(),
                    Some(id),
                );
                if let Some(shell) = session.terminal.shell_program() {
                    review.default_shell = shell.to_string();
                }
                review.destination_shell = session.terminal.shell_program().map(str::to_string);
                review.cwd_is_current = session.terminal.current_directory().is_some();
                self.ui.command_review = Some(review);
            }
        } else {
            if let Err(error) = session.terminal.paste(text) {
                self.ui
                    .show_toast(format!("Terminal paste failed: {error}"));
            }
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::modals::terminal_editor::terminal_editor_test_support::live_editor;

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn terminal_slash_opens_a_live_session_without_a_provider_or_chat_submission() {
        for dismiss_completion in [false, true] {
            let mut config = crate::config::AppConfig::default();
            config.tui.terminal_shell = Some("/bin/sh".into());
            crate::app::tests::with_test_app_config("terminal-slash", config, |app| {
                app.replace_input_content("/terminal".into());
                if dismiss_completion {
                    app.handle_key(key(KeyCode::Esc));
                }
                let action = app
                    .handle_key(key(KeyCode::Enter))
                    .expect("terminal action");
                tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                    .expect("runtime")
                    .block_on(async {
                        app.dispatch(action).await.expect("open terminal");
                        assert!(app.ui.tabs[0].messages.is_empty());
                        assert!(app.ui.tabs[0].input_content.is_empty());
                        assert_eq!(app.ui.terminals.sessions.len(), 1);
                        let session = app
                            .ui
                            .terminals
                            .sessions
                            .values_mut()
                            .next()
                            .expect("session");
                        assert!(!session.terminal.is_done());
                        assert_eq!(session.terminal.shell_program(), Some("/bin/sh"));
                        session.terminal.confirm_close();
                    });
            });
        }
    }

    #[test]
    fn command_review_open_plain_enter_paste_and_cancel_send_nothing() {
        crate::app::tests::with_test_app("command-review-no-auto-run", |app| {
            app.ui.tabs[0].messages.push(crate::app::Message::new(
                0,
                "assistant".into(),
                "```bash\nprintf reviewed\n```\n".into(),
            ));
            app.open_command_review(0);
            assert!(app.ui.command_review.is_some());
            assert!(matches!(app.terminal_key(key(KeyCode::Enter)), Some(None)));
            assert!(app.terminal_paste("unexpected paste\n"));
            assert!(app.ui.terminals.sessions.is_empty());
            app.terminal_key(key(KeyCode::Esc));
            assert!(app.ui.command_review.is_none());
            assert!(app.ui.terminals.sessions.is_empty());
            app.ui.tabs[0].streaming = true;
            app.open_command_review(0);
            assert!(app.ui.command_review.is_none());
        });
    }

    #[test]
    fn same_terminal_survives_view_changes_and_cancelled_close() {
        crate::app::tests::with_test_app("terminal-persists", |app| {
            let (terminal, probe) = live_editor();
            let id = app.ui.terminals.insert(TerminalSession {
                terminal,
                cwd: PathBuf::from("/tmp"),
                host: "test-host".into(),
            });
            app.ui
                .app_tabs
                .open_view(AppView::Terminal { session_id: id });
            app.show_terminal(id, TerminalPlacement::Right);
            app.show_terminal(id, TerminalPlacement::Tab);
            app.show_terminal(id, TerminalPlacement::Bottom);
            assert_eq!(app.ui.terminals.sessions.len(), 1);
            assert_eq!(probe.kills(), 0);
            app.request_terminal_close(id);
            assert_eq!(app.ui.terminals.close_pending, Some(id));
            app.terminal_key(key(KeyCode::Esc));
            assert_eq!(probe.kills(), 0);
            assert!(app.ui.terminals.sessions.contains_key(&id));
            app.request_terminal_close(id);
            app.terminal_key(key(KeyCode::Char('y')));
            assert_eq!(probe.kills(), 1);
            assert!(app.ui.terminals.sessions.is_empty());
        });
    }

    #[test]
    fn terminal_focus_routes_ctrl_c_but_reserves_search_and_focus_keys() {
        crate::app::tests::with_test_app("terminal-key-routing", |app| {
            let (terminal, probe) = live_editor();
            let id = app.ui.terminals.insert(TerminalSession {
                terminal,
                cwd: PathBuf::from("/tmp"),
                host: "test-host".into(),
            });
            app.ui
                .app_tabs
                .open_view(AppView::Terminal { session_id: id });
            app.show_terminal(id, TerminalPlacement::Bottom);
            assert!(app
                .handle_key(KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL))
                .is_none());
            assert!(app.ui.active_modal.is_none());
            assert!(app.ui.tabs[0].input_content.is_empty());
            assert!(matches!(
                app.handle_key(KeyEvent::new(KeyCode::Char('p'), KeyModifiers::CONTROL)),
                Some(Action::OpenCommandPalette)
            ));
            app.handle_key(key(KeyCode::F(6)));
            assert!(!app.ui.terminals.focused);
            assert_eq!(probe.kills(), 0);
        });
    }
}

#[cfg(test)]
mod approval_tests {
    use super::*;

    #[test]
    fn existing_destination_never_receives_approved_run_or_respawns() {
        crate::app::tests::with_test_app("existing-run-is-transfer-only", |app| {
            let (terminal, probe) =
                crate::ui::modals::terminal_editor::terminal_editor_test_support::live_editor();
            let id = app.ui.terminals.insert(TerminalSession {
                terminal,
                cwd: PathBuf::from("/tmp"),
                host: "test-host".into(),
            });
            app.ui.command_review = Some(CommandReview::new(
                vec![crate::ui::command_blocks::CommandBlock {
                    language: "bash".into(),
                    code: "printf should-not-run\n".into(),
                }],
                PathBuf::from("/tmp"),
                "test-host".into(),
                Some(id),
            ));
            tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .expect("runtime")
                .block_on(app.approve_command_review());
            assert_eq!(app.ui.terminals.sessions.len(), 1);
            assert_eq!(probe.kills(), 0);
            assert!(app
                .ui
                .terminals
                .sessions
                .get_mut(&id)
                .expect("original session")
                .terminal
                .take_error()
                .is_none());
            assert!(app.ui.command_review.is_none());
        });
    }
}
