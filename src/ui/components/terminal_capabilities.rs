use std::{fmt::Write, num::NonZeroU16, process::Command, sync::OnceLock};

use ratatui::{
    buffer::{Buffer, CellDiffOption},
    layout::Rect,
    style::{Color, Modifier, Style},
};
use ratatui_image::picker::{cap_parser::QueryStdioOptions, Capability, Picker, ProtocolType};
use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;

static TERMINAL_PICKER: OnceLock<Picker> = OnceLock::new();
const FORCED_CELL_WIDTH: CellDiffOption =
    CellDiffOption::ForcedWidth(NonZeroU16::new(1).expect("forced cell width must be non-zero"));

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

pub fn render_kitty_heading(
    buf: &mut Buffer,
    area: Rect,
    text: &str,
    tier: crate::ui::components::markdown_model::KittyHeadingTier,
    style: Style,
    caps: TerminalCapabilities,
) {
    if area.width == 0 || area.height < 2 || text.trim().is_empty() || !caps.kitty_text_sizing {
        return;
    }
    let sequence = build_kitty_heading_sequence(area.width, text, tier, style, caps);
    if sequence.is_empty() {
        return;
    }

    let first = (area.x, area.y);
    if let Some(cell) = buf.cell_mut(first) {
        cell.set_symbol(&sequence)
            .set_style(style)
            .set_diff_option(FORCED_CELL_WIDTH);
    }

    for y in area.y..area.y.saturating_add(2) {
        for x in area.x..area.x.saturating_add(area.width) {
            if (x, y) == first {
                continue;
            }
            if let Some(cell) = buf.cell_mut((x, y)) {
                cell.set_diff_option(CellDiffOption::Skip);
            }
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

fn build_kitty_heading_sequence(
    width: u16,
    text: &str,
    tier: crate::ui::components::markdown_model::KittyHeadingTier,
    style: Style,
    caps: TerminalCapabilities,
) -> String {
    let mut sequence = String::new();
    let clean_text = text
        .trim()
        .chars()
        .filter(|ch| !ch.is_control())
        .collect::<String>();
    if clean_text.is_empty() {
        return sequence;
    }

    sequence.push_str("\x1b[s\x1b[?7l");
    push_ansi_style(&mut sequence, style);
    write!(sequence, "\x1b[{}X\x1b[1B\x1b[{}X\x1b[1A", width, width).expect("write erase sequence");

    let mut chunk = String::new();
    let mut chunk_columns = 0usize;
    for grapheme in clean_text.graphemes(true) {
        let grapheme_columns = UnicodeWidthStr::width(grapheme).max(1);
        let candidate_bytes = chunk.len() + grapheme.len();
        let would_overflow_columns =
            chunk_columns > 0 && chunk_columns + grapheme_columns > tier.chunk_column_limit();
        let would_overflow_bytes = candidate_bytes > 4000;
        if would_overflow_columns || would_overflow_bytes {
            append_text_sizing_chunk(&mut sequence, &chunk, chunk_columns, tier, caps);
            chunk.clear();
            chunk_columns = 0;
        }
        chunk.push_str(grapheme);
        chunk_columns += grapheme_columns;
    }
    append_text_sizing_chunk(&mut sequence, &chunk, chunk_columns, tier, caps);

    sequence.push_str("\x1b[0m\x1b[?7h\x1b[u\x1b[1C");
    sequence
}

fn append_text_sizing_chunk(
    sequence: &mut String,
    chunk: &str,
    chunk_columns: usize,
    tier: crate::ui::components::markdown_model::KittyHeadingTier,
    caps: TerminalCapabilities,
) {
    if chunk.is_empty() {
        return;
    }
    let osc = tier.osc_sequence(chunk, chunk_columns);
    sequence.push_str(&wrap_for_terminal_passthrough(caps, &osc));
}

fn push_ansi_style(sequence: &mut String, style: Style) {
    if style.add_modifier.contains(Modifier::BOLD) {
        sequence.push_str("\x1b[1m");
    }
    if style.add_modifier.contains(Modifier::ITALIC) {
        sequence.push_str("\x1b[3m");
    }
    if style.add_modifier.contains(Modifier::UNDERLINED) {
        sequence.push_str("\x1b[4m");
    }
    push_ansi_color(sequence, style.fg.unwrap_or(Color::Reset), true);
    push_ansi_color(sequence, style.bg.unwrap_or(Color::Reset), false);
}

fn push_ansi_color(sequence: &mut String, color: Color, foreground: bool) {
    let base = if foreground { 30 } else { 40 };
    let bright = if foreground { 90 } else { 100 };
    match color {
        Color::Reset => sequence.push_str(if foreground { "\x1b[39m" } else { "\x1b[49m" }),
        Color::Black => write!(sequence, "\x1b[{base}m").expect("write ansi color"),
        Color::Red => write!(sequence, "\x1b[{}m", base + 1).expect("write ansi color"),
        Color::Green => write!(sequence, "\x1b[{}m", base + 2).expect("write ansi color"),
        Color::Yellow => write!(sequence, "\x1b[{}m", base + 3).expect("write ansi color"),
        Color::Blue => write!(sequence, "\x1b[{}m", base + 4).expect("write ansi color"),
        Color::Magenta => write!(sequence, "\x1b[{}m", base + 5).expect("write ansi color"),
        Color::Cyan => write!(sequence, "\x1b[{}m", base + 6).expect("write ansi color"),
        Color::Gray => write!(sequence, "\x1b[{}m", base + 7).expect("write ansi color"),
        Color::DarkGray => write!(sequence, "\x1b[{bright}m").expect("write ansi color"),
        Color::LightRed => write!(sequence, "\x1b[{}m", bright + 1).expect("write ansi color"),
        Color::LightGreen => write!(sequence, "\x1b[{}m", bright + 2).expect("write ansi color"),
        Color::LightYellow => write!(sequence, "\x1b[{}m", bright + 3).expect("write ansi color"),
        Color::LightBlue => write!(sequence, "\x1b[{}m", bright + 4).expect("write ansi color"),
        Color::LightMagenta => write!(sequence, "\x1b[{}m", bright + 5).expect("write ansi color"),
        Color::LightCyan => write!(sequence, "\x1b[{}m", bright + 6).expect("write ansi color"),
        Color::White => write!(sequence, "\x1b[{}m", bright + 7).expect("write ansi color"),
        Color::Indexed(index) => {
            write!(
                sequence,
                "\x1b[{};5;{}m",
                if foreground { 38 } else { 48 },
                index
            )
            .expect("write ansi color");
        }
        Color::Rgb(r, g, b) => {
            write!(
                sequence,
                "\x1b[{};2;{};{};{}m",
                if foreground { 38 } else { 48 },
                r,
                g,
                b
            )
            .expect("write ansi color");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::components::markdown_model::KittyHeadingTier;
    use ratatui::{
        buffer::{Buffer, CellDiffOption},
        layout::Rect,
        style::{Color, Style},
    };

    #[test]
    fn kitty_heading_is_anchored_to_the_first_buffer_cell() {
        // Given
        let caps = TerminalCapabilities {
            terminal: TerminalKind::Kitty,
            multiplexer: None,
            kitty_graphics: true,
            kitty_text_sizing: true,
            tmux_passthrough: false,
        };
        let mut buffer = Buffer::empty(Rect::new(0, 0, 20, 4));

        // When
        render_kitty_heading(
            &mut buffer,
            Rect::new(2, 1, 10, 2),
            "Heading",
            KittyHeadingTier::H2,
            Style::default().fg(Color::Cyan).bg(Color::Black),
            caps,
        );

        // Then
        let first = &buffer[(2, 1)];
        assert!(first.symbol().contains("\x1b]66;"));
        assert_eq!(first.diff_option, FORCED_CELL_WIDTH);
        assert_eq!(buffer[(3, 1)].diff_option, CellDiffOption::Skip);
        assert_eq!(buffer[(2, 2)].diff_option, CellDiffOption::Skip);
    }

    #[test]
    fn kitty_heading_sequence_uses_standard_osc_terminator() {
        let sequence = build_kitty_heading_sequence(
            12,
            "Heading",
            KittyHeadingTier::H3,
            Style::default().fg(Color::Green).bg(Color::Black),
            TerminalCapabilities {
                terminal: TerminalKind::Kitty,
                multiplexer: None,
                kitty_graphics: true,
                kitty_text_sizing: true,
                tmux_passthrough: false,
            },
        );

        assert!(sequence.contains("\x1b]66;s=2:n=3:d=4"));
        assert!(sequence.contains("\x1b\\"));
        assert!(!sequence.contains('\u{7}'));
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
