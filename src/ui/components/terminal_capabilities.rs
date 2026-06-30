use std::{io::Write, process::Command, sync::OnceLock};

use crossterm::{
    cursor::{MoveTo, RestorePosition, SavePosition},
    queue,
    style::Print,
};
use ratatui_image::picker::{cap_parser::QueryStdioOptions, Capability, Picker, ProtocolType};

static TERMINAL_PICKER: OnceLock<Picker> = OnceLock::new();

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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KittyTextOverlay {
    pub x: u16,
    pub y: u16,
    pub text: String,
    pub scale: u8,
}

impl TerminalCapabilities {
    pub fn detect() -> Self {
        let terminal = detect_terminal();
        let multiplexer = detect_multiplexer();
        let tmux_passthrough =
            multiplexer == Some(MultiplexerKind::Tmux) && tmux_allows_passthrough();
        let picker = TERMINAL_PICKER.get();
        let kitty_graphics = picker
            .map(|picker| picker.protocol_type() == ProtocolType::Kitty)
            .unwrap_or_else(|| {
                matches!(terminal, TerminalKind::Kitty | TerminalKind::Ghostty)
                    && multiplexer_supports_graphics(multiplexer, tmux_passthrough)
            });
        let kitty_text_sizing = picker.is_some_and(|picker| {
            picker
                .capabilities()
                .contains(&Capability::TextSizingProtocol)
        });

        Self {
            terminal,
            multiplexer,
            kitty_graphics,
            kitty_text_sizing,
            tmux_passthrough,
        }
    }
}

pub fn initialize_terminal_profile() {
    let mut options = QueryStdioOptions::default();
    options.text_sizing_protocol = true;
    let picker =
        Picker::from_query_stdio_with_options(options).unwrap_or_else(|_| Picker::halfblocks());
    let _ = TERMINAL_PICKER.set(picker);
}

pub fn terminal_picker() -> Picker {
    TERMINAL_PICKER
        .get()
        .cloned()
        .unwrap_or_else(Picker::halfblocks)
}

pub fn write_kitty_text_overlays(
    writer: &mut impl Write,
    overlays: &[KittyTextOverlay],
    caps: TerminalCapabilities,
) -> std::io::Result<()> {
    if overlays.is_empty() || !caps.kitty_text_sizing {
        return Ok(());
    }
    queue!(writer, SavePosition)?;
    for overlay in overlays {
        let text = overlay
            .text
            .chars()
            .filter(|ch| !ch.is_control())
            .collect::<String>();
        let sequence = format!("\u{1b}]66;s={};{text}\u{7}", overlay.scale.clamp(1, 7));
        queue!(
            writer,
            MoveTo(overlay.x, overlay.y),
            Print(wrap_for_terminal_passthrough(caps, &sequence))
        )?;
    }
    queue!(writer, RestorePosition)?;
    writer.flush()
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kitty_text_overlay_is_emitted_after_frame_with_cursor_restore() {
        // Given
        let caps = TerminalCapabilities {
            terminal: TerminalKind::Kitty,
            multiplexer: None,
            kitty_graphics: true,
            kitty_text_sizing: true,
            tmux_passthrough: false,
        };
        let overlays = vec![KittyTextOverlay {
            x: 4,
            y: 2,
            text: "Heading".to_string(),
            scale: 2,
        }];
        let mut output = Vec::new();

        // When
        write_kitty_text_overlays(&mut output, &overlays, caps).expect("write overlays");

        // Then
        assert!(output
            .windows(17)
            .any(|window| window == b"\x1b]66;s=2;Heading\x07"));
        assert!(output.starts_with(b"\x1b7"));
        assert!(output.ends_with(b"\x1b8"));
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
