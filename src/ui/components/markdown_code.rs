use std::borrow::Cow;

use ratatui::{
    style::{Modifier, Style},
    text::{Line, Span},
};
use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;

const TAB_STOP: usize = 8;

pub(super) fn render_code(
    lines: &mut Vec<Line<'static>>,
    language: Option<&str>,
    content: &str,
    width: usize,
) {
    let width = width.clamp(1, u16::MAX as usize);
    let theme = crate::theme::active_theme();
    let style = Style::default().fg(theme.code_fg).bg(theme.code_bg);
    let source: Vec<Cow<'_, str>> = content.split('\n').map(expand_line).collect();
    let frame = language
        .is_none_or(|name| {
            ["", "text", "txt", "plain", "plaintext", "ascii"]
                .iter()
                .any(|expected| name.eq_ignore_ascii_case(expected))
        })
        .then(|| Frame::parse(&source))
        .flatten();
    let natural_width = source.iter().map(|line| line.width()).max().unwrap_or(0);
    // Padding must stay proportional to the source, even for huge, mostly empty frames.
    let frame_width = width
        .min(natural_width)
        .min(content.len().saturating_mul(4) / source.len().max(1));
    let framed = frame.is_some() && frame_width >= 4;
    let reflowed = natural_width > if framed { frame_width } else { width };
    if let Some(language) = language.filter(|name| !name.is_empty()) {
        push_wrapped(
            lines,
            &format!(" {language}{} ", if reflowed { " · wrapped" } else { "" }),
            width,
            Style::default()
                .fg(theme.selection_fg)
                .bg(theme.selection_bg)
                .add_modifier(Modifier::BOLD),
            false,
        );
    } else if reflowed {
        push_wrapped(
            lines,
            "[wrapped]",
            width,
            style.add_modifier(Modifier::DIM),
            false,
        );
    }
    if let Some(frame) = frame {
        if framed {
            frame.render(lines, frame_width, style);
        } else {
            // Below four columns there is no room for a frame and its content.
            // Keep all labels/content readable rather than splitting border art.
            push_wrapped(lines, "[box reflowed]", width, style, false);
            push_wrapped(
                lines,
                frame.top.trim_matches(frame.horizontal),
                width,
                style,
                false,
            );
            for row in frame.rows {
                push_wrapped(lines, row, width, style, false);
            }
        }
    } else {
        for line in source {
            push_wrapped(lines, &line, width, style, true);
        }
    }
}

fn expand_line(source: &str) -> Cow<'_, str> {
    if !source.chars().any(char::is_control) {
        return Cow::Borrowed(source);
    }
    let mut result = String::with_capacity(source.len());
    let mut column = 0;
    for grapheme in source.graphemes(true) {
        if grapheme == "\t" {
            let spaces = TAB_STOP - column % TAB_STOP;
            result.extend(std::iter::repeat_n(' ', spaces));
            column += spaces;
        } else if grapheme.chars().any(char::is_control) {
            // Display controls as text, never terminal commands. Copy still uses the source.
            for ch in grapheme.chars() {
                if ch.is_control() {
                    let escaped = ch.escape_default().to_string();
                    column += escaped.width();
                    result.push_str(&escaped);
                } else {
                    column += UnicodeWidthStr::width(ch.encode_utf8(&mut [0; 4]));
                    result.push(ch);
                }
            }
        } else {
            column += grapheme.width();
            result.push_str(grapheme);
        }
    }
    Cow::Owned(result)
}

fn push_wrapped(
    lines: &mut Vec<Line<'static>>,
    text: &str,
    width: usize,
    style: Style,
    continuation: bool,
) {
    let mut rest = text;
    let mut continued = false;
    loop {
        let first_width = rest
            .graphemes(true)
            .next()
            .map_or(0, UnicodeWidthStr::width);
        let prefix = if continued && continuation && width >= first_width + 2 {
            "↪ "
        } else if continued && continuation && width > first_width {
            "↪"
        } else {
            ""
        };
        let available = width - prefix.width();
        let (part, remaining) = split_width(rest, available);
        let mut row = String::from(prefix);
        if part.is_empty() && !remaining.is_empty() {
            // A two-cell grapheme cannot fit a one-cell viewport. Show its lossless
            // Unicode escape rather than splitting it or silently clipping it.
            if let Some(grapheme) = remaining.graphemes(true).next() {
                let escaped: String = grapheme.chars().flat_map(char::escape_unicode).collect();
                push_wrapped(lines, &escaped, width, style, false);
                rest = &remaining[grapheme.len()..];
                if rest.is_empty() {
                    break;
                }
                continued = true;
                continue;
            }
        }
        row.push_str(part);
        lines.push(Line::from(Span::styled(row, style)));
        if remaining.is_empty() {
            break;
        }
        rest = remaining;
        continued = true;
    }
}

fn split_width(text: &str, width: usize) -> (&str, &str) {
    let mut used = 0;
    let mut end = 0;
    for (offset, grapheme) in text.grapheme_indices(true) {
        let cells = grapheme.width();
        if used + cells > width {
            break;
        }
        used += cells;
        end = offset + grapheme.len();
    }
    text.split_at(end)
}

struct Frame<'a> {
    indent: &'a str,
    top: &'a str,
    rows: Vec<&'a str>,
    corners: [char; 4],
    vertical: char,
    horizontal: char,
}

impl<'a> Frame<'a> {
    fn parse(lines: &'a [Cow<'_, str>]) -> Option<Self> {
        if lines.len() < 3 {
            return None;
        }
        let first = lines.first()?.trim_end_matches(' ');
        let start = first.find(|ch| ch != ' ')?;
        let indent = &first[..start];
        let first = &first[start..];
        let (corners, vertical, horizontal) = match first.chars().next()? {
            '┌' => (['┌', '┐', '└', '┘'], '│', '─'),
            '╭' => (['╭', '╮', '╰', '╯'], '│', '─'),
            '╔' => (['╔', '╗', '╚', '╝'], '║', '═'),
            '+' => (['+', '+', '+', '+'], '|', '-'),
            _ => return None,
        };
        let top = first.strip_prefix(corners[0])?.strip_suffix(corners[1])?;
        if !top.contains(horizontal) {
            return None;
        }
        let last = lines.last()?.trim_end_matches(' ').strip_prefix(indent)?;
        let bottom = last.strip_prefix(corners[2])?.strip_suffix(corners[3])?;
        if bottom.is_empty() || !bottom.chars().all(|ch| ch == horizontal) {
            return None;
        }
        let mut rows = Vec::with_capacity(lines.len() - 2);
        for line in &lines[1..lines.len() - 1] {
            let row = line.trim_end_matches(' ').strip_prefix(indent)?;
            let interior = row.strip_prefix(vertical)?.strip_suffix(vertical)?;
            // Multiple columns/nested frames need their own geometry; do not guess.
            if interior.contains(vertical) {
                return None;
            }
            rows.push(interior.trim_end_matches(' '));
        }
        Some(Self {
            indent,
            top,
            rows,
            corners,
            vertical,
            horizontal,
        })
    }

    fn render(self, lines: &mut Vec<Line<'static>>, width: usize, style: Style) {
        // Preserve indentation when it leaves room for a usable frame.
        let indent = if self.indent.width() + 4 <= width {
            self.indent
        } else {
            ""
        };
        let inner_width = width - indent.width() - 2;
        let rule = self.horizontal.to_string().repeat(inner_width);
        let top = resize_rule(self.top, self.horizontal, inner_width);
        lines.push(Line::from(Span::styled(
            format!(
                "{indent}{}{}{}",
                self.corners[0],
                top.as_deref().unwrap_or(&rule),
                self.corners[1]
            ),
            style,
        )));
        if top.is_none() {
            self.render_row(
                lines,
                self.top.trim_end_matches(self.horizontal),
                indent,
                inner_width,
                style,
            );
        }
        for row in &self.rows {
            self.render_row(lines, row, indent, inner_width, style);
        }
        lines.push(Line::from(Span::styled(
            format!("{indent}{}{rule}{}", self.corners[2], self.corners[3]),
            style,
        )));
    }

    fn render_row(
        &self,
        lines: &mut Vec<Line<'static>>,
        row: &str,
        indent: &str,
        width: usize,
        style: Style,
    ) {
        let leading = row.len() - row.trim_start_matches(' ').len();
        let continuation_indent = leading.min(width / 3);
        let mut rest = row;
        let mut continued = false;
        loop {
            let padding = if continued { continuation_indent } else { 0 };
            let available = width - padding;
            let (mut part, mut remaining) = split_width(rest, available);
            if !remaining.is_empty() {
                if let Some(space) = part
                    .rfind(' ')
                    .filter(|&offset| offset > leading && offset > part.len() / 2)
                {
                    (part, remaining) = rest.split_at(space + 1);
                }
            }
            let text = if part.is_empty() && !remaining.is_empty() {
                // Frames have at least two interior cells, so normal terminal
                // graphemes always fit. Escape unusual wider clusters safely.
                let grapheme = remaining.graphemes(true).next().unwrap_or("");
                let escaped: String = grapheme.chars().flat_map(char::escape_unicode).collect();
                self.render_row(lines, &escaped, indent, width, style);
                rest = &remaining[grapheme.len()..];
                if rest.is_empty() {
                    break;
                }
                continued = true;
                continue;
            } else {
                format!("{}{}", " ".repeat(padding), part)
            };
            lines.push(Line::from(Span::styled(
                format!(
                    "{indent}{}{text}{}{}",
                    self.vertical,
                    " ".repeat(width - text.width()),
                    self.vertical
                ),
                style,
            )));
            if remaining.is_empty() {
                break;
            }
            rest = remaining;
            continued = true;
        }
    }
}

fn resize_rule(source: &str, horizontal: char, width: usize) -> Option<String> {
    let source_width = source.width();
    if source_width <= width {
        return Some(format!(
            "{source}{}",
            horizontal.to_string().repeat(width - source_width)
        ));
    }
    let remove = source_width - width;
    if source.chars().filter(|&ch| ch == horizontal).count() < remove {
        return None;
    }
    let mut remaining = remove;
    Some(
        source
            .chars()
            .filter(|&ch| {
                if ch == horizontal && remaining > 0 {
                    remaining -= 1;
                    false
                } else {
                    true
                }
            })
            .collect(),
    )
}
