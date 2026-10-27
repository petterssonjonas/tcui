mod keyboard;
mod plaintext;

use std::collections::{BTreeMap, HashSet};

use crossterm::event::{
    KeyCode, KeyEvent, KeyEventKind, KeyModifiers, MouseButton, MouseEvent, MouseEventKind,
};
use ratatui::{
    layout::{Constraint, Layout, Position, Rect},
    style::{Modifier, Style},
    widgets::{Block, Borders, Paragraph, Wrap},
    Frame,
};
use serde_json::Value;

use crate::{
    config::AppConfig,
    tui::settings_panel::{DangerLevel, Setting, SettingCategory, SettingType},
};

const PUSHOVER_SETUP: &str = "\
Mobile app: https://pushover.net - download and set up.\n\
Needs user key + app token; approve curl in Terminal.\n\
Model key reads disclose keys to your AI provider.\n\
Scheduled keys/messages: private PLAINTEXT copies.\n\
Encrypted sources are copied too.\n\
One-shot: through delivery attempt (success or failure).\n\
Recurring: until cancellation; cancel to remove copies.\n\
Lock/password changes do not revoke scheduled copies.";

#[derive(Debug, Clone, Copy)]
pub enum ValueKind {
    Bool,
    Text,
    OptionalText,
    Number { min: u64, max: u64 },
    Decimal,
    Choice(&'static [&'static str]),
    Lines,
    Structured,
    Manager(&'static str),
}

#[derive(Debug, Clone, Copy)]
pub struct WorkspaceSetting {
    pub id: &'static str,
    pub title: &'static str,
    pub description: &'static str,
    pub category: SettingCategory,
    pub kind: ValueKind,
}

macro_rules! entry {
    ($id:literal, $title:literal, $description:literal, $category:ident, $kind:expr) => {
        WorkspaceSetting {
            id: $id,
            title: $title,
            description: $description,
            category: SettingCategory::$category,
            kind: $kind,
        }
    };
}

/// Persisted compatibility fields without a runtime consumer; not editable controls.
pub const INACTIVE_CONFIG_FIELDS: &[(&str, &str)] = &[
    (
        "small_model",
        "No model selection or request path reads this legacy value.",
    ),
    (
        "tab_configs",
        "Legacy presets are not read by workspace or conversation creation.",
    ),
    (
        "local_inference.model_directory",
        "TCUI connects to servers; it does not launch models from a directory.",
    ),
    (
        "local_inference.health_check_interval_seconds",
        "Connection checks are explicit; there is no scheduled health-check service.",
    ),
    (
        "local_inference.api_token_env",
        "Environment credential autoload is deliberately disabled.",
    ),
    (
        "tui.pinned_commands",
        "The search-only navigator replaces the obsolete pinned-command palette.",
    ),
];

pub fn catalog() -> &'static [WorkspaceSetting] {
    use ValueKind::*;
    &[
        entry!("theme", "Color theme", "Applies immediately across the workspace. Choose a built-in theme.", Theme, Choice(&["system", "gruvbox-dark-low-contrast", "gruvbox-dark-high-contrast", "nord", "dracula", "github", "kanagawa", "catppuccin", "material", "matrix", "monokai", "zenburn", "solarized", "tokyo-night", "opencode"])),
        entry!("default_provider", "Default provider", "Saved connection used for new conversations. Add connections through Providers / Connect first. Existing chats keep their selection.", Provider, Text),
        entry!("providers", "Providers / Connect", "Manage provider name, endpoint, environment-variable reference, backend type and authentication type in the dedicated manager. Secrets stay in masked credential fields.", Provider, Manager("open_providers")),
        entry!("disabled_providers", "Disabled providers", "One provider name per line; spaces are literal. Blank lines are ignored. Escapes: \\\\, \\n, \\r, \\t; \\e means an empty entry. Provider manager also controls visibility.", Provider, Lines),
        entry!("default_model", "Default model", "Model identifier for new conversations using the saved default connection. Empty selects an available model or the local fallback. Existing chats keep their model.", Model, Text),
        entry!("disabled_models", "Disabled models", "One provider:model identifier per line; spaces are literal. Blank lines are ignored. Escapes: \\\\, \\n, \\r, \\t; \\e means an empty entry.", Model, Lines),
        entry!("local_inference.enabled", "Enable local inference", "Make the local inference provider available. Saving does not start a server or execute a command.", Local, Bool),
        entry!("local_inference.scheme", "Local server protocol", "HTTP or HTTPS for the local inference endpoint.", Local, Choice(&["http", "https"])),
        entry!("local_inference.host", "Local server host", "Hostname or IP address only; no scheme, path or credentials.", Local, Text),
        entry!("local_inference.port", "Local server port", "TCP port from 1 to 65535.", Local, Number { min: 1, max: 65535 }),
        entry!("local_inference.server_type", "Local server type", "Select the backend protocol. Auto detects the backend when connecting.", Local, Choice(&["auto", "ollama", "llama_cpp", "lm_studio", "open_ai_compat"])),
        entry!("local_inference.selected_model", "Local fallback model", "Fallback model for new Local Inference conversations when Default model is empty. Does not change running conversations.", Local, Text),
        entry!("local_inference.connect_timeout_ms", "Probe connect timeout (ms)", "Connection timeout for subsequent explicit local server connection checks (minimum 250 ms). Does not change a request already in flight.", Local, Number { min: 250, max: u64::MAX }),
        entry!("local_inference.request_timeout_ms", "Probe request timeout (ms)", "Request timeout for subsequent explicit local server checks (minimum 500 ms). Chat requests use the provider client defaults.", Local, Number { min: 500, max: u64::MAX }),
        entry!("user_alignment", "User message alignment", "Horizontal alignment of user messages.", ChatBehavior, Choice(&["left", "middle", "right"])),
        entry!("ai_alignment", "Assistant message alignment", "Horizontal alignment of assistant messages.", ChatBehavior, Choice(&["left", "middle", "right"])),
        entry!("markdown_mode", "Markdown rendering", "Off renders plain content; full renders rich Markdown; textual keeps terminal-oriented formatting.", ChatBehavior, Choice(&["off", "full", "textual"])),
        entry!("collapse_thinking", "Collapse thinking", "Fold assistant reasoning sections by default.", ChatBehavior, Bool),
        entry!("web_search.enabled", "Automatic web search", "Search before each chat message using the selected source. The query is sent to that service. Explicit @websearch, @research or service mentions work even when automatic search is off.", WebSearch, Bool),
        entry!("web_search.backend", "Web search source", "Local uses built-in keyless search or your SearXNG endpoint. Exa, Tavily and Firecrawl use their enabled MCP profiles and saved API keys. No silent fallback to another source.", WebSearch, Choice(&["local", "exa", "tavily", "firecrawl"])),
        entry!("web_search.max_results", "Search result limit", "Maximum requested search results, from 1 to 10. Tavily currently requires at least five. Service minimums never silently increase your limit.", WebSearch, Number { min: 1, max: 10 }),
        entry!("web_search.local_endpoint", "Local SearXNG endpoint", "Optional HTTP(S) SearXNG search URL, for example http://localhost:8080/search, with JSON output enabled. Empty uses built-in keyless search. Local means the request runs on the TCUI host, including over SSH.", WebSearch, OptionalText),
        entry!("search_api_keys", "Search API keys", "Add or replace Exa, Tavily and Firecrawl API keys; enable or disable their MCP profiles. Keys use the same encrypted vault or plaintext database preference as provider credentials, never environment variables.", WebSearch, Manager("search_api_keys")),
        entry!("show_selector", "Show provider and model selectors", "Display provider and model selectors above the composer.", UiLayout, Bool),
        entry!("show_chat_scrollbar", "Show chat scrollbar", "Display the conversation scrollbar.", UiLayout, Bool),
        entry!("kitty_enhanced_text", "Kitty enhanced text", "Use supported terminal enhanced text rendering. Requires a compatible terminal.", Theme, Bool),
        entry!("kitty_heading_downscale", "Kitty heading downscale", "Reduce enhanced heading sizes by zero, one or two levels.", Theme, Choice(&["none", "one", "two"])),
        entry!("image_protocol", "Image protocol", "Terminal image rendering protocol. Auto detects terminal support; off disables images.", Theme, Choice(&["auto", "off", "kitty", "sixel", "iterm2", "halfblocks"])),
        entry!("quit_confirmation", "Confirm quit", "Request confirmation before closing TCUI.", ChatBehavior, Bool),
        entry!("tui.left_sidebar_mode", "Conversation sidebar mode", "Closed, thin or wide conversation sidebar. Applies immediately.", UiLayout, Choice(&["closed", "thin", "wide"])),
        entry!("tui.right_sidebar_mode", "Files sidebar mode", "Closed, thin or wide files sidebar. Narrow terminals can temporarily hide it.", UiLayout, Choice(&["closed", "thin", "wide"])),
        entry!("tui.left_thin_width", "Conversation thin width", "Sidebar width in columns, 16–48.", UiLayout, Number { min: 16, max: 48 }),
        entry!("tui.left_wide_width", "Conversation wide width", "Sidebar width in columns, 32–64.", UiLayout, Number { min: 32, max: 64 }),
        entry!("tui.right_thin_width", "Files thin width", "Sidebar width in columns, 20–56.", UiLayout, Number { min: 20, max: 56 }),
        entry!("tui.right_wide_width", "Files wide width", "Sidebar width in columns, 40–80.", UiLayout, Number { min: 40, max: 80 }),
        entry!("tui.status_bar_rows", "Status bar rows", "Use one or two status rows.", UiLayout, Number { min: 1, max: 2 }),
        entry!("tui.status_widgets", "Status widget placement", "One widget per line: provider 1 1 (id row area). Rows 1–2, areas 1–6. Empty restores defaults.", UiLayout, Structured),
        entry!("tui.helper_enabled", "Terminal helper hints", "Show or hide the terminal footer keyboard legend. Terminal header controls remain available.", UiLayout, Bool),
        entry!("tui.toast_position", "Toast position", "Place transient notifications, or turn them off.", Notifications, Choice(&["top_left", "top_center", "top_right", "center", "off"])),
        entry!("notifications.desktop", "Desktop notifications", "Allow supported terminal, Herdr or local DBus notification delivery.", Notifications, Bool),
        entry!("notifications.pushover.enabled", "Enable Pushover", "Send mobile notifications and explicit @ping-user messages through curl. Approve curl in Command whitelist first; blacklist overrides approval.", Pushover, Bool),
        entry!("notifications.pushover.use_vault", "Use TCUI-keystore credentials", "On: selected encrypted/plaintext keys only; no fallback. Encrypted keys require unlock. Off: environment/file only.", Pushover, Bool),
        entry!("notifications.pushover.encrypt_credentials", "Encrypt new / replacement keys", "On: password-encrypted keystore. Off: explicit PLAINTEXT SQLite, readable while locked. Applies only when saving a key; siblings are unchanged.", Pushover, Bool),
        entry!("notifications.pushover.application", "Selected application name", "Default tcui; select a saved token or name a new one, then store it below. Use 1-64 lowercase letters, digits, hyphens or underscores.", Pushover, Text),
        entry!("pushover_user_key", "Set / replace user key (masked)", "Use the encryption choice above. Encrypted replacement requires unlock, even when saving plaintext. Other keys stay unchanged.", Pushover, Manager("pushover_user_key")),
        entry!("pushover_app_key", "Set / replace selected app token (masked)", "Save only the selected application's token with the encryption choice above. Other profiles stay unchanged; saved keys are never revealed.", Pushover, Manager("pushover_app_key")),
        entry!("notifications.pushover.env_file", "Environment credential file", "Absolute, user-owned mode-0600 literal KEY=value file, max 64 KiB. Environment overrides file. Ignored in TCUI-keystore mode; never paste keys.", Pushover, OptionalText),
        entry!("commands.whitelist", "Command whitelist", "One rule per line. curl allows all arguments; curl -H requires the exact literal argument prefix -H. Quote values containing spaces. Empty denies all. Never approve a shell to bypass policy.", Terminal, Lines),
        entry!("commands.blacklist", "Command blacklist", "One rule per line. Executable-only rules deny all arguments; argument rules deny the exact literal argv prefix. Quotes preserve spaces. Blacklist wins, including symlinks.", Terminal, Lines),
        entry!("show_memories", "Show memories", "Show memories in the Chat Artifacts sidebar.", Files, Bool),
        entry!("show_vault", "Show Vault", "Show the internal Vault in the Chat Artifacts sidebar.", Files, Bool),
        entry!("credential_vault", "Credential vault", "Unlock, lock and manage credentials in the dedicated encrypted vault. Passwords, API keys and private keys are never shown in settings.", Privacy, Manager("open_credential_vault")),
        entry!("application_protection", "Application protection", "Choose entry password and encryption independently for credentials, chats, memories and the internal vault. Unchecked data stays plaintext. Active requests are cancelled and saved before the application is reconstructed.", Privacy, Manager("open_security")),
        entry!("key_file", "Legacy credential import path", "Optional legacy key-file path, not a secret. Changing this does not import credentials; use the credential manager for migration and unlock.", Privacy, OptionalText),
        entry!("mcp_servers", "MCP server configuration", "Blank line separates servers. Labels: name:, transport: stdio/streamable_http, command:, args: quoted words, url:, enabled: true/false. Omit absent fields; empty removes all. Escapes: \\\\, \\n, \\r, \\t. Save executes nothing.", Mcp, Structured),
        entry!("tui.keybinding_overrides", "Keyboard overrides", "Select an action and press a key chord. Empty restores defaults. Ctrl+P remains the navigator. Reserved editing/navigation chords cannot be overridden.", Keybind, Structured),
        entry!("tui.terminal_placement", "Terminal placement", "Place the terminal below chat, to its right or in its own tab. Applies immediately to the displayed terminal. Sessions remain alive when views change.", Terminal, Choice(&["bottom", "right", "tab"])),
        entry!("tui.terminal_shell", "Default shell", "Shell executable name or absolute path on the execution host; no command-line arguments. Empty uses the platform shell. Applies to new sessions only; saving executes nothing.", Terminal, OptionalText),
        entry!("tui.terminal_start_directory", "Terminal starting directory", "Optional existing absolute directory on the TCUI host (including SSH servers). Empty uses the application directory. Applies to new sessions only.", Terminal, OptionalText),
        #[cfg(feature = "memory")]
        entry!("memory.enabled", "Enable memory", "Allow memory retrieval for subsequent requests.", Memory, Bool),
        #[cfg(feature = "memory")]
        entry!("memory.max_memories", "Maximum recalled memories", "Maximum memory records included in request context.", Memory, Number { min: 0, max: 255 }),
        #[cfg(feature = "memory")]
        entry!("memory.max_context_chars", "Memory context characters", "Maximum characters of memory context included in requests.", Memory, Number { min: 0, max: usize::MAX as u64 }),
        #[cfg(feature = "memory")]
        entry!("memory.min_similarity", "Memory similarity threshold", "Minimum similarity from 0 to 1.", Memory, Decimal),
    ]
}

pub fn navigator_settings() -> impl Iterator<Item = Setting> {
    catalog().iter().map(|entry| Setting {
        id: entry.id,
        title: entry.title,
        description: entry.description,
        category: entry.category,
        setting_type: match entry.kind {
            ValueKind::Manager(command) => SettingType::Command(command),
            _ => SettingType::Text,
        },
        keywords: &[],
        danger: DangerLevel::Safe,
    })
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SettingsIntent {
    Apply { id: String, value: String },
    Command(&'static str),
    Save { draft: Option<(String, String)> },
}

#[derive(Debug, Clone)]
struct EditValue {
    id: &'static str,
    text: String,
    cursor: usize,
}

#[derive(Debug, Clone)]
pub struct SettingsWorkspaceState {
    config: Value,
    pub category: SettingCategory,
    pub selected: usize,
    rail_focused: bool,
    edit: Option<EditValue>,
    keyboard: keyboard::KeyboardSettings,
    pub feedback: String,
    pub has_error: bool,
    category_areas: Vec<(Rect, SettingCategory)>,
    row_areas: Vec<(Rect, usize)>,
    list_height: usize,
    save_area: Rect,
}

impl Default for SettingsWorkspaceState {
    fn default() -> Self {
        Self {
            config: serde_json::to_value(AppConfig::default()).unwrap_or(Value::Null),
            category: SettingCategory::Theme,
            selected: 0,
            rail_focused: false,
            edit: None,
            keyboard: keyboard::KeyboardSettings::default(),
            feedback: "Changes apply individually. Save persists all current settings and a valid open draft.".into(),
            has_error: false,
            category_areas: Vec::new(),
            row_areas: Vec::new(),
            list_height: 1,
            save_area: Rect::default(),
        }
    }
}

impl SettingsWorkspaceState {
    pub fn load(&mut self, config: &AppConfig) {
        match serde_json::to_value(config) {
            Ok(value) => self.config = value,
            Err(error) => self.fail(format!("Cannot load settings: {error}")),
        }
    }

    pub fn select_id(&mut self, id: &str) {
        if let Some(action) = id.strip_prefix("keybind:") {
            if let Some(selected) = self.keyboard.select_action(action) {
                self.category = SettingCategory::Keybind;
                self.selected = selected;
                self.rail_focused = false;
            }
            return;
        }
        if let Some(setting) = catalog().iter().find(|entry| entry.id == id) {
            self.category = setting.category;
            self.selected = self.rows().position(|entry| entry.id == id).unwrap_or(0);
            self.rail_focused = false;
        }
    }

    pub fn fail(&mut self, message: impl Into<String>) {
        self.feedback = message.into();
        self.has_error = true;
        if self.category == SettingCategory::Keybind {
            self.keyboard.fail(self.feedback.clone());
        }
    }

    pub fn applied(&mut self, config: &AppConfig, id: &str) {
        self.load(config);
        if self.edit.as_ref().is_some_and(|edit| edit.id == id) {
            self.edit = None;
        }
        if id == "tui.keybinding_overrides" {
            self.keyboard.applied();
        }
        self.feedback = "Applied and saved. Defaults affect subsequent operations; existing chats, terminal sessions and other drafts are preserved.".into();
        self.has_error = false;
    }

    fn rows(&self) -> impl Iterator<Item = &'static WorkspaceSetting> + Clone + 'static {
        let category = self.category;
        catalog()
            .iter()
            .filter(move |entry| entry.category == category)
    }

    fn categories() -> &'static [SettingCategory] {
        use SettingCategory::*;
        &[
            Theme,
            Terminal,
            UiLayout,
            ChatBehavior,
            WebSearch,
            Provider,
            Model,
            Local,
            Notifications,
            Pushover,
            Files,
            Keybind,
            Mcp,
            Privacy,
            #[cfg(feature = "memory")]
            Memory,
        ]
    }

    fn move_selection(&mut self, delta: isize) {
        if self.rail_focused {
            let categories = Self::categories();
            let index = categories
                .iter()
                .position(|category| *category == self.category)
                .unwrap_or(0);
            let index = index
                .saturating_add_signed(delta)
                .min(categories.len().saturating_sub(1));
            self.category = categories[index];
            self.selected = 0;
        } else {
            self.selected = self
                .selected
                .saturating_add_signed(delta)
                .min(self.row_count().saturating_sub(1));
        }
    }

    fn keyboard_bindings(&self) -> BTreeMap<String, String> {
        get_value(&self.config, "tui.keybinding_overrides")
            .and_then(|value| serde_json::from_value(value.clone()).ok())
            .unwrap_or_default()
    }

    fn row_count(&self) -> usize {
        if self.category == SettingCategory::Keybind {
            self.keyboard.count()
        } else {
            self.rows().count()
        }
    }

    fn activate(&mut self, direction: isize) -> Option<SettingsIntent> {
        if self.category == SettingCategory::Keybind && !self.keyboard.activate(self.selected) {
            return None;
        }
        let setting = if self.category == SettingCategory::Keybind {
            self.rows().next()?
        } else {
            self.rows().nth(self.selected)?
        };
        let current = get_value(&self.config, setting.id);
        match setting.kind {
            ValueKind::Manager(command) => Some(SettingsIntent::Command(command)),
            ValueKind::Bool => Some(SettingsIntent::Apply {
                id: setting.id.into(),
                value: (!current.and_then(Value::as_bool).unwrap_or(false)).to_string(),
            }),
            ValueKind::Choice(choices) => {
                let index = choices
                    .iter()
                    .position(|choice| Some(*choice) == current.and_then(Value::as_str))
                    .unwrap_or(0);
                let index =
                    (index as isize + direction).rem_euclid(choices.len() as isize) as usize;
                Some(SettingsIntent::Apply {
                    id: setting.id.into(),
                    value: choices[index].into(),
                })
            }
            _ => {
                let text = match plaintext::display(&self.config, setting) {
                    Ok(text) => text,
                    Err(error) => {
                        self.fail(error);
                        return None;
                    }
                };
                self.edit = Some(EditValue {
                    id: setting.id,
                    cursor: text.len(),
                    text,
                });
                self.feedback = "Draft: Ctrl+S or Save validates and saves; Esc cancels; Ctrl+U clears. Multiline: Enter adds a line; Ctrl+Enter applies.".into();
                self.has_error = false;
                None
            }
        }
    }

    pub fn paste(&mut self, text: &str) {
        if let Some(edit) = &mut self.edit {
            // Paste is data only, never a command or an apply gesture.
            let text: String = text
                .replace("\r\n", "\n")
                .chars()
                .filter(|character| !character.is_control() || matches!(character, '\n' | '\t'))
                .collect();
            edit.text.insert_str(edit.cursor, &text);
            edit.cursor += text.len();
        }
    }

    pub fn handle_key(&mut self, key: KeyEvent) -> Option<SettingsIntent> {
        if key.kind == KeyEventKind::Release {
            return None;
        }
        if key.code == KeyCode::Char('s')
            && key.modifiers == KeyModifiers::CONTROL
            && !self.keyboard.is_capturing()
        {
            return self.save_intent();
        }
        if self.edit.is_none() && self.category == SettingCategory::Keybind && !self.rail_focused {
            let bindings = self.keyboard_bindings();
            if let Some(intent) = self.keyboard.handle_key(key, &mut self.selected, &bindings) {
                return intent;
            }
        }
        if let Some(edit) = self.edit.as_mut() {
            match key.code {
                KeyCode::Esc => {
                    self.edit = None;
                    self.feedback = "Draft cancelled; saved value unchanged.".into();
                    self.has_error = false;
                }
                KeyCode::Enter
                    if !key.modifiers.contains(KeyModifiers::CONTROL)
                        && catalog()
                            .iter()
                            .find(|entry| entry.id == edit.id)
                            .is_some_and(plaintext::multiline) =>
                {
                    edit.text.insert(edit.cursor, '\n');
                    edit.cursor += 1;
                }
                KeyCode::Enter => {
                    let id = edit.id;
                    let setting = catalog().iter().find(|entry| entry.id == id)?;
                    let text = match plaintext::serialized(setting, &edit.text) {
                        Ok(text) => text,
                        Err(error) => {
                            self.fail(error);
                            return None;
                        }
                    };
                    return Some(SettingsIntent::Apply {
                        id: id.into(),
                        value: text,
                    });
                }
                KeyCode::Char('u') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                    edit.text.clear();
                    edit.cursor = 0;
                }
                KeyCode::Char(c)
                    if !key
                        .modifiers
                        .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT) =>
                {
                    edit.text.insert(edit.cursor, c);
                    edit.cursor += c.len_utf8();
                }
                KeyCode::Backspace if edit.cursor > 0 => {
                    let previous = edit.text[..edit.cursor]
                        .char_indices()
                        .next_back()
                        .map_or(0, |(i, _)| i);
                    edit.text.drain(previous..edit.cursor);
                    edit.cursor = previous;
                }
                KeyCode::Delete if edit.cursor < edit.text.len() => {
                    let next = edit.cursor
                        + edit.text[edit.cursor..]
                            .chars()
                            .next()
                            .map_or(0, char::len_utf8);
                    edit.text.drain(edit.cursor..next);
                }
                KeyCode::Left => {
                    edit.cursor = edit.text[..edit.cursor]
                        .char_indices()
                        .next_back()
                        .map_or(0, |(i, _)| i)
                }
                KeyCode::Right if edit.cursor < edit.text.len() => {
                    edit.cursor += edit.text[edit.cursor..]
                        .chars()
                        .next()
                        .map_or(0, char::len_utf8)
                }
                KeyCode::Up => edit.cursor = plaintext::move_line(&edit.text, edit.cursor, false),
                KeyCode::Down => edit.cursor = plaintext::move_line(&edit.text, edit.cursor, true),
                KeyCode::Home => {
                    edit.cursor = edit.text[..edit.cursor]
                        .rfind('\n')
                        .map_or(0, |index| index + 1)
                }
                KeyCode::End => {
                    edit.cursor += edit.text[edit.cursor..]
                        .find('\n')
                        .unwrap_or(edit.text.len() - edit.cursor)
                }
                _ => {}
            }
            return None;
        }
        match key.code {
            KeyCode::Tab | KeyCode::BackTab => self.rail_focused = !self.rail_focused,
            KeyCode::Up | KeyCode::Char('k') => self.move_selection(-1),
            KeyCode::Down | KeyCode::Char('j') => self.move_selection(1),
            KeyCode::PageUp => self.move_selection(-(self.list_height as isize)),
            KeyCode::PageDown => self.move_selection(self.list_height as isize),
            KeyCode::Home => self.move_selection(-(self.row_count() as isize)),
            KeyCode::End => self.move_selection(self.row_count() as isize),
            KeyCode::Left if self.rail_focused => self.move_selection(-1),
            KeyCode::Right if self.rail_focused => self.move_selection(1),
            KeyCode::Left => return self.activate(-1),
            KeyCode::Right | KeyCode::Enter | KeyCode::Char(' ') if !self.rail_focused => {
                return self.activate(1)
            }
            KeyCode::Enter => self.rail_focused = false,
            KeyCode::Esc => self.rail_focused = true,
            _ => {}
        }
        None
    }

    fn save_intent(&mut self) -> Option<SettingsIntent> {
        let pending = self.keyboard.pending_draft();
        if self.keyboard.is_capturing() && pending.is_none() {
            self.fail("Finish or cancel the keyboard capture before saving.");
            return None;
        }
        let draft = if let Some(edit) = &self.edit {
            let setting = catalog().iter().find(|entry| entry.id == edit.id)?;
            match plaintext::serialized(setting, &edit.text) {
                Ok(text) => Some((edit.id.to_string(), text)),
                Err(error) => {
                    self.fail(error);
                    return None;
                }
            }
        } else {
            pending
        };
        Some(SettingsIntent::Save { draft })
    }

    pub fn handle_mouse(&mut self, event: MouseEvent) -> Option<SettingsIntent> {
        if matches!(event.kind, MouseEventKind::Down(MouseButton::Left))
            && self
                .save_area
                .contains(Position::new(event.column, event.row))
        {
            return self.save_intent();
        }
        if self.edit.is_some()
            || (self.category == SettingCategory::Keybind && self.keyboard.is_capturing())
        {
            return None;
        }
        let position = Position::new(event.column, event.row);
        if self.category == SettingCategory::Keybind
            && matches!(event.kind, MouseEventKind::Down(MouseButton::Left))
        {
            let bindings = self.keyboard_bindings();
            if let Some(intent) = self.keyboard.reset_at(position, &bindings) {
                return intent;
            }
        }
        match event.kind {
            MouseEventKind::Down(MouseButton::Left) => {
                if let Some((_, category)) = self
                    .category_areas
                    .iter()
                    .find(|(rect, _)| rect.contains(position))
                {
                    self.category = *category;
                    self.selected = 0;
                    self.rail_focused = false;
                } else if let Some((_, index)) = self
                    .row_areas
                    .iter()
                    .find(|(rect, _)| rect.contains(position))
                {
                    self.selected = *index;
                    self.rail_focused = false;
                    return self.activate(1);
                }
            }
            MouseEventKind::ScrollUp => self.move_selection(-1),
            MouseEventKind::ScrollDown => self.move_selection(1),
            _ => {}
        }
        None
    }

    pub fn render(&mut self, frame: &mut Frame, area: Rect) {
        self.category_areas.clear();
        self.row_areas.clear();
        self.save_area = Rect::default();
        if area.is_empty() {
            return;
        }
        let theme = crate::theme::active_theme();
        let base = Style::default().fg(theme.foreground).bg(theme.background);
        frame.render_widget(Block::default().style(base), area);
        let [title, body, description, feedback, help] = Layout::vertical([
            Constraint::Length(1),
            Constraint::Min(1),
            Constraint::Length(if area.height >= 16 { 4 } else { 2 }),
            Constraint::Length(if self.has_error && area.height >= 16 {
                4
            } else {
                2
            }),
            Constraint::Length(1),
        ])
        .areas(area);
        let [heading, save] =
            Layout::horizontal([Constraint::Min(0), Constraint::Length(16)]).areas(title);
        self.save_area = save;
        frame.render_widget(
            Paragraph::new(" Settings ").style(base.fg(theme.accent).add_modifier(Modifier::BOLD)),
            heading,
        );
        frame.render_widget(
            Paragraph::new("[ Save Ctrl+S ]")
                .style(base.fg(theme.accent).add_modifier(Modifier::REVERSED)),
            save,
        );
        let narrow = area.width < 76;
        let [rail, list] = if narrow {
            Layout::vertical([Constraint::Length(3), Constraint::Min(1)]).areas(body)
        } else {
            Layout::horizontal([Constraint::Length(22), Constraint::Min(1)]).areas(body)
        };
        let categories = Self::categories();
        let category_index = categories
            .iter()
            .position(|category| *category == self.category)
            .unwrap_or(0);
        let rail_block = Block::default()
            .borders(Borders::ALL)
            .title(if self.rail_focused {
                " Categories [focus] "
            } else {
                " Categories "
            });
        let rail_inner = rail_block.inner(rail);
        frame.render_widget(rail_block, rail);
        if narrow {
            frame.render_widget(
                Paragraph::new(format!(
                    "‹ {} ›  ({}/{})",
                    self.category.label(),
                    category_index + 1,
                    categories.len()
                ))
                .style(base.fg(theme.accent)),
                rail_inner,
            );
            let half = rail_inner.width / 2;
            if category_index > 0 {
                self.category_areas.push((
                    Rect::new(rail_inner.x, rail_inner.y, half, rail_inner.height),
                    categories[category_index - 1],
                ));
            }
            if category_index + 1 < categories.len() {
                self.category_areas.push((
                    Rect::new(
                        rail_inner.x + half,
                        rail_inner.y,
                        rail_inner.width - half,
                        rail_inner.height,
                    ),
                    categories[category_index + 1],
                ));
            }
        } else {
            let offset =
                category_index.saturating_sub(rail_inner.height.saturating_sub(1) as usize);
            for (index, category) in categories
                .iter()
                .enumerate()
                .skip(offset)
                .take(rail_inner.height as usize)
            {
                let rect = Rect::new(
                    rail_inner.x,
                    rail_inner.y + (index - offset) as u16,
                    rail_inner.width,
                    1,
                );
                let style = if *category == self.category {
                    base.fg(theme.accent).add_modifier(Modifier::BOLD)
                } else {
                    base
                };
                frame.render_widget(
                    Paragraph::new(format!(" {}", category.label())).style(style),
                    rect,
                );
                self.category_areas.push((rect, *category));
            }
        }
        let list = if self.category == SettingCategory::Pushover {
            let notice = Paragraph::new(PUSHOVER_SETUP)
                .wrap(Wrap { trim: true })
                .style(base.fg(theme.warning));
            let notice_height =
                (notice.line_count(list.width) as u16).min(list.height.saturating_sub(3));
            let [notice_area, controls] =
                Layout::vertical([Constraint::Length(notice_height), Constraint::Min(1)])
                    .areas(list);
            frame.render_widget(notice, notice_area);
            controls
        } else {
            list
        };
        if self.category == SettingCategory::Keybind && self.edit.is_none() {
            self.selected = self.selected.min(self.keyboard.count().saturating_sub(1));
            self.list_height = list.height.saturating_sub(2) as usize;
            let bindings = self.keyboard_bindings();
            self.keyboard.render(
                frame,
                [list, description, feedback, help],
                self.selected,
                &bindings,
                !self.rail_focused,
                &mut self.row_areas,
            );
            return;
        }
        let rows = self.rows();
        let row_count = rows.clone().count();
        self.selected = self.selected.min(row_count.saturating_sub(1));
        let block = Block::default().borders(Borders::ALL).title(format!(
            " {} · {}/{} ",
            self.category.label(),
            self.selected + 1,
            row_count
        ));
        let inner = block.inner(list);
        frame.render_widget(block, list);
        self.list_height = inner.height as usize;
        let offset = self
            .selected
            .saturating_sub(self.list_height.saturating_sub(1));
        for (index, setting) in rows.clone().enumerate().skip(offset).take(self.list_height) {
            let rect = Rect::new(inner.x, inner.y + (index - offset) as u16, inner.width, 1);
            let style = if index == self.selected && !self.rail_focused {
                base.fg(theme.accent).add_modifier(Modifier::REVERSED)
            } else {
                base
            };
            let value = match setting.kind {
                ValueKind::Manager(_) => "[Open manager]".into(),
                ValueKind::Lines | ValueKind::Structured => "[Enter to edit entries]".into(),
                _ => value_text(get_value(&self.config, setting.id)),
            };
            frame.render_widget(
                Paragraph::new(format!(" {}  {}", setting.title, value)).style(style),
                rect,
            );
            self.row_areas.push((rect, index));
        }
        if let Some(setting) = rows.clone().nth(self.selected) {
            frame.render_widget(
                Paragraph::new(if self.category == SettingCategory::Pushover {
                    format!("{}\n{}", setting.title, setting.description)
                } else {
                    format!(
                        "{} · {}\n{}",
                        setting.title, setting.id, setting.description
                    )
                })
                .wrap(Wrap { trim: true })
                .style(base),
                description,
            );
        }
        if let Some(edit) = &self.edit {
            let block = Block::default()
                .borders(Borders::ALL)
                .title(" Draft · Ctrl+S saves · Esc cancels ");
            let input = block.inner(list);
            frame.render_widget(ratatui::widgets::Clear, list);
            frame.render_widget(block.style(base), list);
            let before = &edit.text[..edit.cursor];
            let row = before.bytes().filter(|byte| *byte == b'\n').count();
            let line_start = before.rfind('\n').map_or(0, |index| index + 1);
            let column = unicode_width::UnicodeWidthStr::width(&before[line_start..]);
            let top = row.saturating_sub(input.height.saturating_sub(1) as usize);
            let left = column.saturating_sub(input.width.saturating_sub(1) as usize);
            frame.render_widget(
                Paragraph::new(edit.text.as_str())
                    .style(base.fg(theme.warning))
                    .scroll((
                        top.min(u16::MAX as usize) as u16,
                        left.min(u16::MAX as usize) as u16,
                    )),
                input,
            );
            if !input.is_empty() {
                frame.set_cursor_position((
                    input.x + (column - left) as u16,
                    input.y + (row - top) as u16,
                ));
            }
            frame.render_widget(
                Paragraph::new(self.feedback.as_str())
                    .wrap(Wrap { trim: true })
                    .style(base.fg(if self.has_error {
                        theme.error
                    } else {
                        theme.warning
                    })),
                feedback,
            );
        } else {
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
        }
        frame.render_widget(Paragraph::new("Ctrl+S Save · Tab categories · ↑↓ move · Enter edit/newline · Ctrl+Enter apply · Esc cancel").style(base), help);
    }
}

fn get_value<'a>(value: &'a Value, id: &str) -> Option<&'a Value> {
    id.split('.').try_fold(value, |value, key| value.get(key))
}
fn value_text(value: Option<&Value>) -> String {
    match value {
        None | Some(Value::Null) => String::new(),
        Some(Value::String(text)) => text.clone(),
        Some(value) => value.to_string(),
    }
}

pub fn validate_edit(config: &Value, id: &str, text: &str) -> Result<AppConfig, String> {
    let entry = catalog()
        .iter()
        .find(|entry| entry.id == id)
        .ok_or_else(|| {
            INACTIVE_CONFIG_FIELDS
                .iter()
                .find(|(field, _)| *field == id)
                .map_or_else(
                    || "Unknown setting".to_string(),
                    |(_, reason)| format!("Inactive compatibility setting: {reason}"),
                )
        })?;
    let value = match entry.kind {
        ValueKind::Manager(_) => return Err("Use the dedicated manager for this setting".into()),
        ValueKind::Bool => Value::Bool(text.parse().map_err(|_| "Expected true or false")?),
        ValueKind::Number { min, max } => {
            let number: u64 = text
                .trim()
                .parse()
                .map_err(|_| "Expected a whole non-negative number")?;
            if !(min..=max).contains(&number) {
                return Err(format!("Value must be between {min} and {max}"));
            }
            number.into()
        }
        ValueKind::Decimal => {
            let number: f64 = text
                .trim()
                .parse()
                .map_err(|_| "Expected a number between 0 and 1")?;
            if !number.is_finite() || !(0.0..=1.0).contains(&number) {
                return Err("Similarity must be between 0 and 1".into());
            }
            Value::from(number)
        }
        ValueKind::Choice(choices) => {
            if !choices.contains(&text) {
                return Err(format!("Choose one of: {}", choices.join(", ")));
            }
            text.into()
        }
        ValueKind::Lines | ValueKind::Structured => {
            serde_json::from_str(text).map_err(|error| format!("Invalid JSON: {error}"))?
        }
        ValueKind::OptionalText if text.trim().is_empty() => Value::Null,
        ValueKind::Text | ValueKind::OptionalText => text.trim().into(),
    };
    if text.chars().any(|c| {
        c.is_control()
            && (!matches!(entry.kind, ValueKind::Lines | ValueKind::Structured)
                || !matches!(c, '\n' | '\t'))
    }) {
        return Err("Control characters are not allowed in text values".into());
    }
    let allowed_fields: Option<&[&str]> = match id {
        "mcp_servers" => Some(&["name", "transport", "command", "args", "url", "enabled"]),
        "tui.status_widgets" => Some(&["id", "row", "area"]),
        _ => None,
    };
    if let Some(allowed) = allowed_fields {
        for entry in value.as_array().ok_or("Expected a JSON array")? {
            let object = entry
                .as_object()
                .ok_or("Expected JSON objects in the array")?;
            if let Some(unknown) = object.keys().find(|key| !allowed.contains(&key.as_str())) {
                return Err(format!(
                    "Unknown field {unknown}; allowed fields: {}",
                    allowed.join(", ")
                ));
            }
        }
    }
    let mut updated = config.clone();
    let mut keys = id.split('.').peekable();
    let mut destination = &mut updated;
    while let Some(key) = keys.next() {
        if keys.peek().is_none() {
            destination[key] = value;
            break;
        }
        destination = destination
            .get_mut(key)
            .ok_or("Missing configuration section")?;
    }
    let mut candidate: AppConfig = serde_json::from_value(updated.clone())
        .map_err(|error| format!("Invalid setting: {error}"))?;
    // TUI deserialization clamps old files, but an interactive edit must never silently clamp.
    if id == "tui.status_widgets" {
        let input = get_value(&updated, id).ok_or("Missing status widgets")?;
        let actual = serde_json::to_value(&candidate.tui.status_widgets)
            .map_err(|error| error.to_string())?;
        if *input != actual {
            return Err("Each widget needs id, row (1–2) and area (1–6)".into());
        }
        let mut slots = HashSet::new();
        for widget in &candidate.tui.status_widgets {
            if ![
                "web_search",
                "provider",
                "model",
                "reasoning",
                "connection",
                "context",
                "hints",
                "mcps",
                "tools",
            ]
            .contains(&widget.id.as_str())
            {
                return Err("Unknown widget ID; use web_search, provider, model, reasoning, connection, context, hints or tools".into());
            }
            if !slots.insert((widget.row, widget.area)) {
                return Err("Two widgets cannot occupy the same status slot".into());
            }
        }
    }
    match id {
        "commands.whitelist" | "commands.blacklist" => {
            let entries = if id == "commands.whitelist" {
                &candidate.commands.whitelist
            } else {
                &candidate.commands.blacklist
            };
            for program in entries {
                crate::commands::validate_rule(program)?;
            }
        }
        "notifications.pushover.application" => {
            crate::notifications::pushover::application_key_id(text.trim())
                .map_err(|error| error.to_string())?;
        }
        "notifications.pushover.env_file" if !text.trim().is_empty() => {
            if !std::path::Path::new(text.trim()).is_absolute() {
                return Err("Enter an absolute credential file path, not credentials".into());
            }
        }
        "web_search.enabled"
        | "web_search.backend"
        | "web_search.max_results"
        | "web_search.local_endpoint" => {
            candidate.web_search.validate().map_err(str::to_string)?;
        }
        "default_provider" if text.trim().is_empty() => {
            return Err("Choose a saved connection".into())
        }
        "default_provider" | "default_model" => {
            if !saved_provider(&candidate, &candidate.default_provider) {
                return Err("Choose a saved connection from Providers / Connect before setting a default provider or model".into());
            }
        }
        "local_inference.host"
            if text.trim().is_empty() || text.contains(['/', '@', ' ', '\n']) =>
        {
            return Err("Enter a hostname or IP address without scheme, path or credentials".into())
        }
        "tui.terminal_shell" if !text.trim().is_empty() => {
            if text.chars().any(char::is_whitespace)
                || text.contains([';', '|', '&', '$', '`', '\n'])
            {
                return Err("Enter one executable name or path, without shell arguments".into());
            }
            if text.contains('/')
                && (!std::path::Path::new(text).is_absolute()
                    || !std::path::Path::new(text).is_file())
            {
                return Err("Shell path must name an existing absolute executable file".into());
            }
            #[cfg(unix)]
            if text.contains('/') {
                use std::os::unix::fs::PermissionsExt;
                let metadata = std::fs::metadata(text).map_err(|error| error.to_string())?;
                if metadata.permissions().mode() & 0o111 == 0 {
                    return Err("Shell file must be executable".into());
                }
            }
        }
        "tui.terminal_start_directory" if !text.trim().is_empty() => {
            if !std::path::Path::new(text).is_absolute() || !std::path::Path::new(text).is_dir() {
                return Err("Enter an existing absolute directory on the TCUI host".into());
            }
        }
        "key_file" if !text.trim().is_empty() && !std::path::Path::new(text).is_absolute() => {
            return Err("Enter an absolute path".into())
        }
        "tui.keybinding_overrides" => {
            validate_bindings(&candidate.tui.keybinding_overrides)?;
            for binding in candidate.tui.keybinding_overrides.values_mut() {
                *binding = keyboard::canonical(binding)?;
            }
        }
        "mcp_servers" => {
            let mut names = HashSet::new();
            for server in &candidate.mcp_servers {
                let name = crate::mcp::lookup_profile(&server.name)
                    .map_or(server.name.trim(), |profile| profile.skill);
                if name.is_empty() || !names.insert(name) {
                    return Err("MCP server names must be non-empty and unique".into());
                }
                if !["stdio", "streamable_http"].contains(&server.transport.as_str()) {
                    return Err("MCP transport must be stdio or streamable_http".into());
                }
                if server.transport == "stdio"
                    && server
                        .command
                        .as_deref()
                        .is_none_or(|command| command.trim().is_empty())
                {
                    return Err("A stdio MCP server requires a command".into());
                }
                if server.transport != "stdio"
                    && server.url.as_deref().is_none_or(|url| {
                        !url.starts_with("http://") && !url.starts_with("https://")
                    })
                {
                    return Err("An HTTP MCP server requires an http:// or https:// URL".into());
                }
                if server.transport != "stdio" {
                    validate_url(
                        server
                            .url
                            .as_deref()
                            .ok_or("An HTTP MCP server requires a URL")?,
                    )?;
                }
            }
        }
        _ => {}
    }
    Ok(candidate)
}

fn saved_provider(config: &AppConfig, provider: &str) -> bool {
    !config
        .disabled_providers
        .iter()
        .any(|name| name.eq_ignore_ascii_case(provider))
        && (config
            .providers
            .iter()
            .any(|entry| entry.name.eq_ignore_ascii_case(provider))
            || (config.local_inference.enabled && crate::llm::local::is_local_provider(provider)))
}

fn validate_url(value: &str) -> Result<(), String> {
    let url = reqwest::Url::parse(value).map_err(|_| "Enter a valid absolute HTTP(S) URL")?;
    if !matches!(url.scheme(), "http" | "https")
        || url.host_str().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
    {
        return Err("Use an HTTP(S) URL without embedded credentials".into());
    }
    Ok(())
}

fn validate_bindings(bindings: &BTreeMap<String, String>) -> Result<(), String> {
    keyboard::validate(bindings)
}

pub fn keybinding_action(
    key: KeyEvent,
    bindings: &BTreeMap<String, String>,
) -> Option<crate::app::Action> {
    if key.kind == KeyEventKind::Release {
        return None;
    }
    let repr = crate::tui::keybind_capture::key_repr(key);
    let (id, _) = bindings
        .iter()
        .find(|(_, binding)| binding.eq_ignore_ascii_case(&repr))?;
    keyboard::commands()
        .into_iter()
        .find(|command| command.id == *id)
        .map(|command| command.action)
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::{backend::TestBackend, Terminal};

    #[test]
    fn stored_string_lists_round_trip_without_normalization_or_entry_loss() {
        let entries = vec![
            " model ",
            "",
            "other\nvariant",
            "\\e",
            "line\rreturn",
            "\ttab",
            "   ",
        ];
        for id in ["disabled_models", "disabled_providers"] {
            let setting = catalog()
                .iter()
                .find(|entry| entry.id == id)
                .expect("setting");
            let mut source = config();
            source[id] = serde_json::json!(entries);
            let text = plaintext::display(&source, setting).expect("open");
            assert!(text.contains("\\e"));
            let serialized = plaintext::serialized(setting, &text).expect("serialize");
            assert_eq!(
                serde_json::from_str::<Vec<String>>(&serialized).expect("entries"),
                entries
            );
            let candidate = validate_edit(&source, id, &serialized).expect("valid");
            assert_eq!(
                get_value(&serde_json::to_value(candidate).expect("config"), id),
                source.get(id)
            );
            assert_eq!(
                plaintext::serialized(setting, "").expect("empty list"),
                "[]"
            );
            assert_eq!(
                plaintext::serialized(setting, "\\e").expect("empty entry"),
                "[\"\"]"
            );
        }
    }

    #[test]
    fn failed_keyboard_reset_is_saved_by_mouse_and_keyboard_until_success() {
        let mut state = SettingsWorkspaceState::default();
        let mut config = AppConfig::default();
        config
            .tui
            .keybinding_overrides
            .insert("new_chat".into(), "alt+n".into());
        state.load(&config);
        state.select_id("keybind:new_chat");
        assert!(matches!(
            state.handle_key(KeyEvent::new(KeyCode::Backspace, KeyModifiers::NONE)),
            Some(SettingsIntent::Apply { .. })
        ));
        state.fail("disk full");
        assert!(!state.keyboard.is_capturing());
        let keyboard = state.handle_key(KeyEvent::new(KeyCode::Char('s'), KeyModifiers::CONTROL));
        assert_eq!(
            keyboard,
            Some(SettingsIntent::Save {
                draft: Some(("tui.keybinding_overrides".into(), "{}".into()))
            })
        );
        let mut terminal = Terminal::new(TestBackend::new(80, 24)).expect("terminal");
        terminal
            .draw(|frame| state.render(frame, frame.area()))
            .expect("render");
        assert!(terminal.backend().to_string().contains("Save Ctrl+S"));
        let mouse = state.handle_mouse(MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: state.save_area.x,
            row: state.save_area.y,
            modifiers: KeyModifiers::NONE,
        });
        assert_eq!(mouse, keyboard);
        assert!(state.keyboard.pending_draft().is_some());
        config.tui.keybinding_overrides.clear();
        state.applied(&config, "tui.keybinding_overrides");
        assert_eq!(
            state.handle_key(KeyEvent::new(KeyCode::Char('s'), KeyModifiers::CONTROL)),
            Some(SettingsIntent::Save { draft: None })
        );
    }

    #[test]
    fn plaintext_collections_round_trip_and_validate_every_editable_field() {
        for (id, text) in [
            ("disabled_providers", "one\ntwo"),
            ("disabled_models", "one:model\ntwo:model"),
            ("commands.blacklist", "curl -H 'X-Name: value'"),
            ("tui.status_widgets", "provider 1 1\nmcps 2 6"),
            ("tui.keybinding_overrides", "new_chat=alt+n"),
            ("mcp_servers", "name: server\ntransport: stdio\ncommand: program\nargs: 'one two' '' \"quote'value\"\nurl: https://example.org/mcp\nenabled: true"),
        ] {
            let setting = catalog().iter().find(|setting| setting.id == id).expect("setting");
            let serialized = plaintext::serialized(setting, text).expect("plaintext");
            let candidate = validate_edit(&config(), id, &serialized).expect("valid edit");
            let value = serde_json::to_value(candidate).expect("config");
            let reopened = plaintext::display(&value, setting).expect("display");
            let serialized = plaintext::serialized(setting, &reopened).expect("round trip");
            let next = validate_edit(&value, id, &serialized).expect("round trip valid");
            assert_eq!(get_value(&value, id), get_value(&serde_json::to_value(next).expect("config"), id));
        }
        for (id, invalid) in [
            ("tui.status_widgets", "provider 4 1"),
            ("tui.status_widgets", "provider 1 1\nmodel 1 1"),
            ("tui.keybinding_overrides", "new_chat=ctrl+p"),
            (
                "mcp_servers",
                "name: server\ntransport: stdio\ncommand: server\ntypo: true",
            ),
            (
                "mcp_servers",
                "name: server\ntransport: stdio\ncommand: server\nargs: 'unclosed",
            ),
        ] {
            let setting = catalog()
                .iter()
                .find(|setting| setting.id == id)
                .expect("setting");
            assert!(plaintext::serialized(setting, invalid)
                .and_then(|text| validate_edit(&config(), id, &text))
                .is_err());
        }
    }

    fn config() -> Value {
        serde_json::to_value(AppConfig::default()).expect("config")
    }

    #[test]
    fn command_policy_edits_validate_executable_and_literal_argument_rules() {
        for id in ["commands.whitelist", "commands.blacklist"] {
            assert!(validate_edit(&config(), id, r#"["curl","/usr/bin/printf"]"#).is_ok());
            for invalid in [
                r#""curl""#,
                r#"["curl 'unclosed"]"#,
                r#"["./curl"]"#,
                r#"[""]"#,
                r#"[1]"#,
            ] {
                assert!(validate_edit(&config(), id, invalid).is_err());
            }
        }
        for invalid in ["", "Uppercase", "other app", "../other", &"a".repeat(65)] {
            assert!(
                validate_edit(&config(), "notifications.pushover.application", invalid).is_err()
            );
        }
        assert_eq!(
            validate_edit(
                &config(),
                "notifications.pushover.application",
                "work_alerts"
            )
            .expect("named application")
            .notifications
            .pushover
            .application,
            "work_alerts"
        );
        assert!(
            validate_edit(&config(), "notifications.pushover.env_file", "relative.env").is_err()
        );
        assert!(validate_edit(
            &config(),
            "notifications.pushover.env_file",
            "/private/pushover.env"
        )
        .is_ok());
    }

    #[test]
    fn every_serialized_knob_is_catalogued_or_deliberately_managed() {
        let config = config();
        let managed = ["providers", "saved_files_dir"];
        fn leaves(value: &Value, prefix: String, found: &mut Vec<String>) {
            if let Value::Object(fields) = value {
                for (key, value) in fields {
                    let path = if prefix.is_empty() {
                        key.clone()
                    } else {
                        format!("{prefix}.{key}")
                    };
                    if value.is_object() && !value.as_object().expect("object").is_empty() {
                        leaves(value, path, found);
                    } else {
                        found.push(path);
                    }
                }
            }
        }
        let mut paths = Vec::new();
        leaves(&config, String::new(), &mut paths);
        for path in paths {
            assert!(
                catalog()
                    .iter()
                    .any(|entry| path == entry.id || path.starts_with(&format!("{}.", entry.id)))
                    || managed.contains(&path.as_str())
                    || INACTIVE_CONFIG_FIELDS
                        .iter()
                        .any(|(id, _)| path == *id || path.starts_with(&format!("{id}."))),
                "Missing setting {path}"
            );
        }
    }

    #[test]
    fn numeric_edits_reject_clamping_and_wrong_types() {
        assert!(validate_edit(&config(), "tui.left_thin_width", "99").is_err());
        assert!(validate_edit(&config(), "local_inference.port", "0").is_err());
        assert_eq!(
            validate_edit(&config(), "tui.left_thin_width", "32")
                .expect("valid")
                .tui
                .left_thin_width,
            32
        );
        assert!(validate_edit(
            &config(),
            "tui.status_widgets",
            r#"[{"id":"model","row":4,"area":1}]"#
        )
        .is_err());
    }

    #[test]
    fn settings_never_accept_unknown_or_manager_mutation() {
        assert!(validate_edit(&config(), "providers", "{}").is_err());
        assert!(validate_edit(&config(), "not_a_setting", "true").is_err());
        assert!(validate_edit(
            &config(),
            "tui.keybinding_overrides",
            r#"{"quit":"ctrl+p"}"#
        )
        .is_err());
    }

    #[test]
    fn defaults_cannot_reference_unsaved_provider_connections() {
        assert!(validate_edit(&config(), "default_provider", "OpenAI").is_err());
        assert!(validate_edit(&config(), "default_model", "model-id").is_err());
        let mut config = AppConfig::default();
        config.providers.push(crate::config::ProviderConfig {
            name: "Saved".into(),
            ..Default::default()
        });
        let mut saved = serde_json::to_value(&config).expect("config");
        let updated = validate_edit(&saved, "default_provider", "Saved").expect("saved provider");
        saved = serde_json::to_value(updated).expect("updated");
        assert!(validate_edit(&saved, "default_model", "manual-model").is_ok());
    }

    #[test]
    fn invalid_numeric_draft_reaches_live_validation_and_stays_visible_on_rejection() {
        let mut state = SettingsWorkspaceState::default();
        state.select_id("local_inference.port");
        state.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        state.handle_key(KeyEvent::new(KeyCode::Char('u'), KeyModifiers::CONTROL));
        state.paste("not-a-port");
        assert!(matches!(
            state.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)),
            Some(SettingsIntent::Apply { id, value }) if id == "local_inference.port" && value == "not-a-port"
        ));
        state.fail("Expected a whole non-negative number");
        assert!(state.has_error);
        assert_eq!(
            state.edit.as_ref().expect("retained draft").text,
            "not-a-port"
        );
    }

    #[test]
    fn keyboard_overrides_dispatch_and_reserve_palette_shortcut() {
        let updated = validate_edit(
            &config(),
            "tui.keybinding_overrides",
            r#"{"new_chat":"alt+n"}"#,
        )
        .expect("override");
        assert!(matches!(
            keybinding_action(
                KeyEvent::new(KeyCode::Char('n'), KeyModifiers::ALT),
                &updated.tui.keybinding_overrides
            ),
            Some(crate::app::Action::NewChat)
        ));
        assert!(validate_edit(
            &config(),
            "tui.keybinding_overrides",
            r#"{"new_chat":"ctrl+p"}"#
        )
        .is_err());
    }

    #[test]
    fn text_edit_cancel_keeps_saved_value() {
        let mut state = SettingsWorkspaceState::default();
        state.select_id("default_model");
        state.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        state.handle_key(KeyEvent::new(KeyCode::Char('x'), KeyModifiers::NONE));
        state.handle_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
        assert!(state.edit.is_none());
        assert_eq!(
            get_value(&state.config, "default_model"),
            Some(&Value::from("gpt-4o"))
        );
    }

    #[test]
    fn failed_apply_preserves_draft_for_retry() {
        let mut state = SettingsWorkspaceState::default();
        state.select_id("default_model");
        state.activate(1);
        state.fail("disk full");
        assert!(state.edit.is_some());
        assert!(state.has_error);
    }

    #[test]
    fn duplicate_search_profiles_and_unsupported_transports_are_rejected() {
        assert!(validate_edit(
            &config(), "mcp_servers",
            r#"[{"name":"Exa","transport":"stdio","command":"server"},{"name":"exa","transport":"stdio","command":"server"}]"#
        ).is_err());
        assert!(validate_edit(
            &config(),
            "mcp_servers",
            r#"[{"name":"server","transport":"sse","url":"https://example.org/mcp"}]"#
        )
        .is_err());
    }

    #[test]
    fn sensitive_collections_are_concealed_until_explicitly_opened() {
        let mut state = SettingsWorkspaceState::default();
        state.config["mcp_servers"] = serde_json::json!([{"name": "private", "transport": "streamable_http", "url": "https://example.invalid/secret-canary"}]);
        state.select_id("mcp_servers");
        let mut terminal = Terminal::new(TestBackend::new(100, 30)).expect("terminal");
        terminal
            .draw(|frame| state.render(frame, frame.area()))
            .expect("render");
        assert!(!terminal.backend().to_string().contains("secret-canary"));
        state.activate(1);
        assert!(state
            .edit
            .as_ref()
            .expect("explicit editor")
            .text
            .contains("secret-canary"));
    }

    #[test]
    fn structured_edits_reject_ignored_fields_and_text_rejects_control_characters() {
        assert!(validate_edit(
            &config(),
            "mcp_servers",
            r#"[{"name":"server","transport":"stdio","command":"server","typo":true}]"#
        )
        .is_err());
        assert!(validate_edit(&config(), "local_inference.host", "host\nname").is_err());
        assert!(validate_edit(&config(), "tui.terminal_shell", "sh -c command").is_err());
    }

    #[test]
    fn narrow_and_tiny_layouts_render_without_secret_values() {
        for (width, height) in [(100, 30), (42, 16), (20, 8), (1, 1)] {
            let mut terminal = Terminal::new(TestBackend::new(width, height)).expect("terminal");
            let mut state = SettingsWorkspaceState::default();
            state.select_id("mcp_servers");
            terminal
                .draw(|frame| state.render(frame, frame.area()))
                .expect("render");
            assert!(state
                .row_areas
                .iter()
                .all(|(rect, _)| rect.right() <= width && rect.bottom() <= height));
        }
    }
}
