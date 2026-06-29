use std::process::Command;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TerminalKind {
    Kitty,
    Ghostty,
    WezTerm,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MultiplexerKind {
    Tmux,
    Zellij,
    Herdr,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TerminalCapabilities {
    pub terminal: TerminalKind,
    pub multiplexer: Option<MultiplexerKind>,
    pub kitty_graphics: bool,
    pub kitty_text_sizing: bool,
    pub tmux_passthrough: bool,
}

impl TerminalCapabilities {
    pub fn detect() -> Self {
        let terminal = detect_terminal();
        let multiplexer = detect_multiplexer();
        let tmux_passthrough =
            multiplexer == Some(MultiplexerKind::Tmux) && tmux_allows_passthrough();
        let kitty_graphics = matches!(terminal, TerminalKind::Kitty | TerminalKind::Ghostty)
            && multiplexer_supports_graphics(multiplexer, tmux_passthrough);
        let kitty_text_sizing = terminal == TerminalKind::Kitty
            && multiplexer_supports_graphics(multiplexer, tmux_passthrough);

        Self {
            terminal,
            multiplexer,
            kitty_graphics,
            kitty_text_sizing,
            tmux_passthrough,
        }
    }
}

pub fn wrap_for_terminal_passthrough(caps: TerminalCapabilities, sequence: &str) -> String {
    if caps.multiplexer == Some(MultiplexerKind::Tmux) && caps.tmux_passthrough {
        let escaped = sequence.replace('\u{1b}', "\u{1b}\u{1b}");
        format!("\u{1b}Ptmux;{}\u{1b}\\", escaped)
    } else {
        sequence.to_string()
    }
}

fn detect_terminal() -> TerminalKind {
    let term = std::env::var("TERM").unwrap_or_default();
    let term_program = std::env::var("TERM_PROGRAM").unwrap_or_default();

    if term.contains("kitty") || std::env::var_os("KITTY_WINDOW_ID").is_some() {
        return TerminalKind::Kitty;
    }
    if term_program.eq_ignore_ascii_case("ghostty")
        || std::env::var_os("GHOSTTY_RESOURCES_DIR").is_some()
        || std::env::var_os("GHOSTTY_BIN_DIR").is_some()
    {
        return TerminalKind::Ghostty;
    }
    if term_program.eq_ignore_ascii_case("wezterm") {
        return TerminalKind::WezTerm;
    }

    TerminalKind::Unknown
}

fn detect_multiplexer() -> Option<MultiplexerKind> {
    if std::env::var_os("TMUX").is_some() {
        return Some(MultiplexerKind::Tmux);
    }
    if std::env::var_os("ZELLIJ").is_some() || std::env::var_os("ZELLIJ_SESSION_NAME").is_some() {
        return Some(MultiplexerKind::Zellij);
    }
    if std::env::vars_os().any(|(key, _)| key.to_string_lossy().starts_with("HERDR")) {
        return Some(MultiplexerKind::Herdr);
    }

    None
}

fn multiplexer_supports_graphics(
    multiplexer: Option<MultiplexerKind>,
    tmux_passthrough: bool,
) -> bool {
    match multiplexer {
        None => true,
        Some(MultiplexerKind::Tmux) => tmux_passthrough,
        Some(MultiplexerKind::Zellij | MultiplexerKind::Herdr) => false,
    }
}

fn tmux_allows_passthrough() -> bool {
    Command::new("tmux")
        .args(["show", "-Apv", "allow-passthrough"])
        .output()
        .ok()
        .filter(|output| output.status.success())
        .and_then(|output| String::from_utf8(output.stdout).ok())
        .map(|value| {
            let value = value.trim();
            value == "on" || value == "all"
        })
        .unwrap_or(false)
}
