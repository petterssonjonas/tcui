use pulldown_cmark::{Event, Options, Parser, Tag, TagEnd};
use ratatui::{
    style::{Modifier, Style},
    text::{Line, Span},
};
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

use crate::config::app_config::{HeadingDownscale, MarkdownMode};
use crate::ui::components::terminal_capabilities::TerminalCapabilities;

#[derive(Debug, Clone)]
pub struct RenderedMarkdown {
    pub lines: Vec<Line<'static>>,
    pub link_targets: Vec<LinkTarget>,
    pub images: Vec<RenderedImage>,
    pub kitty_headings: Vec<RenderedHeading>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LinkTarget {
    pub line: usize,
    pub column: usize,
    pub width: usize,
    pub target: String,
}

#[derive(Debug, Clone)]
pub struct RenderedImage {
    pub start_line: usize,
    pub height: usize,
    pub source: String,
}

#[derive(Debug, Clone)]
pub struct RenderedHeading {
    pub start_line: usize,
    pub text: String,
    pub tier: KittyHeadingTier,
    pub style: Style,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KittyHeadingTier {
    H1,
    H2,
    H3,
}

impl KittyHeadingTier {
    pub fn rendered_width(self, text: &str) -> usize {
        let text_width = UnicodeWidthStr::width(text).max(1);
        match self {
            KittyHeadingTier::H1 => text_width.saturating_mul(2),
            KittyHeadingTier::H2 => text_width.saturating_mul(10).div_ceil(6),
            KittyHeadingTier::H3 => text_width.saturating_mul(3).div_ceil(2),
        }
    }

    pub const fn chunk_column_limit(self) -> usize {
        match self {
            KittyHeadingTier::H1 => 128,
            KittyHeadingTier::H2 => 6,
            KittyHeadingTier::H3 => 4,
        }
    }

    pub fn osc_sequence(self, text: &str, columns: usize) -> String {
        match self {
            KittyHeadingTier::H1 => {
                format!("\x1b]66;s=2:w={columns};{text}\x1b\\")
            }
            KittyHeadingTier::H2 => {
                let width = columns.saturating_mul(5).div_ceil(6);
                format!("\x1b]66;s=2:n=5:d=6:w={width};{text}\x1b\\")
            }
            KittyHeadingTier::H3 => {
                let width = columns.saturating_mul(3).div_ceil(4);
                format!("\x1b]66;s=2:n=3:d=4:w={width};{text}\x1b\\")
            }
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct RenderOptions {
    pub mode: MarkdownMode,
    pub width: usize,
    pub kitty_enhanced_text: bool,
    pub kitty_heading_downscale: HeadingDownscale,
    pub image_protocol_enabled: bool,
    pub terminal_capabilities: TerminalCapabilities,
}

#[derive(Debug)]
enum Block {
    Heading {
        level: usize,
        text: String,
    },
    Paragraph(String),
    List {
        ordered: bool,
        items: Vec<String>,
    },
    Quote(Vec<String>),
    Code {
        language: Option<String>,
        content: String,
    },
    Table(Vec<Vec<String>>),
    Rule,
    MediaImage {
        alt: String,
        source: String,
    },
}

#[derive(Debug, Clone)]
struct StyledRun {
    text: String,
    style: Style,
    link: Option<String>,
}

pub fn render_markdown(content: &str, opts: RenderOptions) -> RenderedMarkdown {
    if opts.mode == MarkdownMode::Off {
        let mut link_targets = Vec::new();
        let lines = content
            .lines()
            .enumerate()
            .map(|(line_idx, line)| {
                let runs = plain_runs_with_skills(line);
                append_link_targets(&mut link_targets, line_idx, &runs);
                owned_line(runs)
            })
            .collect();
        return RenderedMarkdown {
            lines,
            link_targets,
            images: Vec::new(),
            kitty_headings: Vec::new(),
        };
    }

    let blocks = parse_blocks(content);
    let mut lines = Vec::new();
    let mut link_targets = Vec::new();
    let mut images = Vec::new();
    let mut kitty_headings = Vec::new();

    for block in blocks {
        let theme = crate::theme::active_theme();
        let start_line = lines.len();
        match block {
            Block::Heading { level, text } => {
                if let Some(mut heading) = render_heading(&mut lines, &text, level, opts) {
                    heading.start_line = start_line;
                    kitty_headings.push(heading);
                }
            }
            Block::Paragraph(text) => {
                render_inline_block(&mut lines, &mut link_targets, &text, opts, Style::default())
            }
            Block::List { ordered, items } => {
                render_list(&mut lines, &mut link_targets, ordered, &items, opts)
            }
            Block::Quote(items) => render_quote(&mut lines, &mut link_targets, &items, opts),
            Block::Code { language, content } => {
                render_code(&mut lines, language.as_deref(), &content, opts.width)
            }
            Block::Table(rows) => render_table(&mut lines, rows, opts.width),
            Block::Rule => lines.push(Line::from("─".repeat(opts.width.max(3)))),
            Block::MediaImage { alt, source } => {
                let image_height = opts.width.clamp(4, 12);
                lines.push(Line::from(vec![
                    Span::styled(
                        "[image] ",
                        Style::default().fg(theme.info).add_modifier(Modifier::BOLD),
                    ),
                    Span::styled(
                        if alt.is_empty() { source.clone() } else { alt },
                        Style::default().fg(theme.foreground),
                    ),
                ]));
                if opts.mode == MarkdownMode::Full && opts.image_protocol_enabled {
                    for _ in 0..image_height {
                        lines.push(Line::from(""));
                    }
                    images.push(RenderedImage {
                        start_line: start_line + 1,
                        height: image_height,
                        source: source.clone(),
                    });
                }
                lines.push(Line::from(vec![
                    Span::styled(
                        "open ",
                        Style::default()
                            .fg(theme.accent)
                            .add_modifier(Modifier::UNDERLINED),
                    ),
                    Span::styled(
                        source.clone(),
                        Style::default()
                            .fg(theme.accent)
                            .add_modifier(Modifier::UNDERLINED),
                    ),
                ]));
                link_targets.push(LinkTarget {
                    line: lines.len().saturating_sub(1),
                    column: 0,
                    width: UnicodeWidthStr::width(format!("open {source}").as_str()),
                    target: source,
                });
            }
        }
        if !matches!(lines.last(), Some(last) if last.spans.is_empty()) {
            lines.push(Line::from(""));
        }
    }

    if matches!(lines.last(), Some(last) if last.spans.is_empty()) {
        lines.pop();
    }

    RenderedMarkdown {
        lines,
        link_targets,
        images,
        kitty_headings,
    }
}

fn plain_runs_with_skills(text: &str) -> Vec<StyledRun> {
    skill_segments(text)
        .into_iter()
        .map(|(text, skill)| StyledRun {
            style: if skill.is_some() {
                Style::default()
                    .fg(crate::theme::active_theme().accent)
                    .add_modifier(Modifier::UNDERLINED)
            } else {
                Style::default()
            },
            link: skill.map(|name| format!("skill:{name}")),
            text,
        })
        .collect()
}

fn skill_segments(text: &str) -> Vec<(String, Option<String>)> {
    let chars: Vec<char> = text.chars().collect();
    let mut segments = Vec::new();
    let mut plain = String::new();
    let mut idx = 0usize;
    let mut in_code = false;

    while idx < chars.len() {
        if chars[idx] == '`' {
            in_code = !in_code;
            plain.push(chars[idx]);
            idx += 1;
            continue;
        }
        let boundary = idx == 0
            || chars[idx - 1].is_whitespace()
            || matches!(chars[idx - 1], '(' | '[' | '{' | '"' | '\'');
        if chars[idx] != '@' || in_code || !boundary {
            plain.push(chars[idx]);
            idx += 1;
            continue;
        }

        let start = idx;
        idx += 1;
        while idx < chars.len()
            && (chars[idx].is_ascii_alphanumeric() || matches!(chars[idx], '-' | '_'))
        {
            idx += 1;
        }
        if idx == start + 1 {
            plain.push('@');
            continue;
        }
        if !plain.is_empty() {
            segments.push((std::mem::take(&mut plain), None));
        }
        let mention: String = chars[start..idx].iter().collect();
        let name: String = chars[start + 1..idx].iter().collect();
        segments.push((mention, Some(name)));
    }

    if !plain.is_empty() {
        segments.push((plain, None));
    }
    segments
}

fn parse_blocks(content: &str) -> Vec<Block> {
    let mut blocks = Vec::new();
    let lines: Vec<&str> = content.lines().collect();
    let mut idx = 0usize;

    while idx < lines.len() {
        let line = lines[idx].trim_end();
        if line.trim().is_empty() {
            idx += 1;
            continue;
        }
        if let Some((fence, language)) = parse_fence_start(line) {
            let mut body = Vec::new();
            idx += 1;
            while idx < lines.len() && !lines[idx].trim_start().starts_with(fence) {
                body.push(lines[idx]);
                idx += 1;
            }
            idx += usize::from(idx < lines.len());
            blocks.push(Block::Code {
                language,
                content: body.join("\n"),
            });
            continue;
        }
        if let Some((level, text)) = parse_heading(line) {
            blocks.push(Block::Heading { level, text });
            idx += 1;
            continue;
        }
        if is_rule(line) {
            blocks.push(Block::Rule);
            idx += 1;
            continue;
        }
        if let Some((alt, source)) = parse_image_line(line) {
            blocks.push(Block::MediaImage { alt, source });
            idx += 1;
            continue;
        }
        if is_table_header(line, lines.get(idx + 1).copied()) {
            let mut rows = vec![split_table_row(line)];
            idx += 2;
            while idx < lines.len() && lines[idx].contains('|') && !lines[idx].trim().is_empty() {
                rows.push(split_table_row(lines[idx]));
                idx += 1;
            }
            blocks.push(Block::Table(rows));
            continue;
        }
        if line.trim_start().starts_with('>') {
            let mut quote_lines = Vec::new();
            while idx < lines.len() && lines[idx].trim_start().starts_with('>') {
                quote_lines.push(
                    lines[idx]
                        .trim_start()
                        .trim_start_matches('>')
                        .trim_start()
                        .to_string(),
                );
                idx += 1;
            }
            blocks.push(Block::Quote(quote_lines));
            continue;
        }
        if parse_list_item(line).is_some() {
            let ordered = line
                .trim_start()
                .chars()
                .next()
                .is_some_and(|c| c.is_ascii_digit());
            let mut items = Vec::new();
            while idx < lines.len() {
                if let Some(item) = parse_list_item(lines[idx]) {
                    items.push(item);
                    idx += 1;
                } else {
                    break;
                }
            }
            blocks.push(Block::List { ordered, items });
            continue;
        }

        let mut paragraph = vec![line.trim().to_string()];
        idx += 1;
        while idx < lines.len()
            && !lines[idx].trim().is_empty()
            && parse_heading(lines[idx]).is_none()
            && parse_list_item(lines[idx]).is_none()
            && parse_fence_start(lines[idx]).is_none()
            && !lines[idx].trim_start().starts_with('>')
            && !is_rule(lines[idx])
        {
            paragraph.push(lines[idx].trim().to_string());
            idx += 1;
        }
        blocks.push(Block::Paragraph(paragraph.join(" ")));
    }

    blocks
}

fn render_inline_block(
    lines: &mut Vec<Line<'static>>,
    link_targets: &mut Vec<LinkTarget>,
    text: &str,
    opts: RenderOptions,
    base_style: Style,
) {
    let wrapped = wrap_runs(parse_inline_runs(text, base_style), opts.width.max(1));
    for (line_idx, line) in wrapped.iter().enumerate() {
        append_link_targets(link_targets, lines.len() + line_idx, line);
    }
    lines.extend(wrapped.into_iter().map(owned_line));
}

fn render_list(
    lines: &mut Vec<Line<'static>>,
    link_targets: &mut Vec<LinkTarget>,
    ordered: bool,
    items: &[String],
    opts: RenderOptions,
) {
    for (idx, item) in items.iter().enumerate() {
        let theme = crate::theme::active_theme();
        let prefix = if ordered {
            format!("{}. ", idx + 1)
        } else {
            "- ".to_string()
        };
        let mut rendered = wrap_runs(
            parse_inline_runs(item, Style::default()),
            opts.width.saturating_sub(prefix.len()).max(1),
        );
        for (line_idx, line) in rendered.iter_mut().enumerate() {
            if line_idx == 0 {
                line.insert(
                    0,
                    StyledRun {
                        text: prefix.clone(),
                        style: Style::default()
                            .fg(theme.warning)
                            .add_modifier(Modifier::BOLD),
                        link: None,
                    },
                );
            } else {
                line.insert(
                    0,
                    StyledRun {
                        text: "  ".to_string(),
                        style: Style::default(),
                        link: None,
                    },
                );
            }
            append_link_targets(link_targets, lines.len() + line_idx, line);
        }
        lines.extend(rendered.into_iter().map(owned_line));
    }
}

fn render_quote(
    lines: &mut Vec<Line<'static>>,
    link_targets: &mut Vec<LinkTarget>,
    items: &[String],
    opts: RenderOptions,
) {
    for item in items {
        let theme = crate::theme::active_theme();
        let mut rendered = wrap_runs(
            parse_inline_runs(item, Style::default().fg(theme.muted)),
            opts.width.saturating_sub(2).max(1),
        );
        for (line_idx, line) in rendered.iter_mut().enumerate() {
            line.insert(
                0,
                StyledRun {
                    text: "| ".to_string(),
                    style: Style::default().fg(theme.border),
                    link: None,
                },
            );
            append_link_targets(link_targets, lines.len() + line_idx, line);
        }
        lines.extend(rendered.into_iter().map(owned_line));
    }
}

fn render_code(
    lines: &mut Vec<Line<'static>>,
    language: Option<&str>,
    content: &str,
    width: usize,
) {
    let theme = crate::theme::active_theme();
    if let Some(language) = language.filter(|language| !language.is_empty()) {
        lines.push(Line::from(Span::styled(
            format!(" {language} "),
            Style::default()
                .fg(theme.selection_fg)
                .bg(theme.selection_bg)
                .add_modifier(Modifier::BOLD),
        )));
    }
    for line in content.lines() {
        let mut text = line.to_string();
        if UnicodeWidthStr::width(text.as_str()) > width {
            text.truncate(width);
        }
        lines.push(Line::from(Span::styled(
            text,
            Style::default().fg(theme.code_fg).bg(theme.code_bg),
        )));
    }
}

fn render_table(lines: &mut Vec<Line<'static>>, rows: Vec<Vec<String>>, width: usize) {
    if rows.is_empty() {
        return;
    }
    let col_count = rows.iter().map(Vec::len).max().unwrap_or(0);
    let col_width = (width.saturating_sub(col_count + 1) / col_count.max(1)).max(3);
    for (idx, row) in rows.iter().enumerate() {
        let mut rendered = String::from("|");
        for cell in row {
            let mut cell = cell.trim().to_string();
            if UnicodeWidthStr::width(cell.as_str()) > col_width {
                cell.truncate(col_width.saturating_sub(1));
                cell.push('…');
            }
            rendered.push_str(&format!(" {:width$}|", cell, width = col_width));
        }
        lines.push(Line::from(rendered));
        if idx == 0 {
            lines.push(Line::from(format!(
                "|{}|",
                "─".repeat((col_width + 2) * row.len() + row.len().saturating_sub(1))
            )));
        }
    }
}

fn render_heading(
    lines: &mut Vec<Line<'static>>,
    text: &str,
    level: usize,
    opts: RenderOptions,
) -> Option<RenderedHeading> {
    let text = text.trim();
    let fallback = heading_style(level);
    lines.push(Line::from(Span::styled(text.to_string(), fallback)));
    let tier = enhanced_heading_tier(level, opts.kitty_heading_downscale)?;
    if !(opts.kitty_enhanced_text
        && opts.terminal_capabilities.kitty_text_sizing
        && opts.mode == MarkdownMode::Full)
    {
        return None;
    }
    if tier.rendered_width(text) > opts.width.max(1) {
        return None;
    }
    lines.push(Line::from(""));
    Some(RenderedHeading {
        start_line: 0,
        text: text.to_string(),
        tier,
        style: heading_overlay_style(level),
    })
}

fn parse_inline_runs(text: &str, base_style: Style) -> Vec<StyledRun> {
    let theme = crate::theme::active_theme();
    let parser = Parser::new_ext(text, Options::ENABLE_STRIKETHROUGH);
    let mut runs = Vec::new();
    let mut styles = vec![base_style];
    let mut links = Vec::new();

    for event in parser {
        match event {
            Event::Start(Tag::Strong) => {
                styles.push(current_style(&styles).add_modifier(Modifier::BOLD))
            }
            Event::End(TagEnd::Strong) => {
                styles.pop();
            }
            Event::Start(Tag::Emphasis) => {
                styles.push(current_style(&styles).add_modifier(Modifier::ITALIC))
            }
            Event::End(TagEnd::Emphasis) => {
                styles.pop();
            }
            Event::Start(Tag::Strikethrough) => {
                styles.push(current_style(&styles).add_modifier(Modifier::CROSSED_OUT))
            }
            Event::End(TagEnd::Strikethrough) => {
                styles.pop();
            }
            Event::Start(Tag::Link { dest_url, .. }) => links.push(dest_url.to_string()),
            Event::End(TagEnd::Link) => {
                links.pop();
            }
            Event::Code(code) => runs.push(StyledRun {
                text: code.to_string(),
                style: current_style(&styles).fg(theme.warning).bg(theme.code_bg),
                link: links.last().cloned(),
            }),
            Event::Text(text) => {
                if let Some(link) = links.last() {
                    runs.push(StyledRun {
                        text: text.to_string(),
                        style: current_style(&styles)
                            .fg(theme.accent)
                            .add_modifier(Modifier::UNDERLINED),
                        link: Some(link.clone()),
                    });
                } else {
                    runs.extend(
                        skill_segments(&text)
                            .into_iter()
                            .map(|(text, skill)| StyledRun {
                                style: if skill.is_some() {
                                    current_style(&styles)
                                        .fg(theme.accent)
                                        .add_modifier(Modifier::UNDERLINED)
                                } else {
                                    current_style(&styles)
                                },
                                link: skill.map(|name| format!("skill:{name}")),
                                text,
                            }),
                    );
                }
            }
            Event::SoftBreak | Event::HardBreak => runs.push(StyledRun {
                text: "\n".to_string(),
                style: current_style(&styles),
                link: None,
            }),
            _ => {}
        }
    }

    runs
}

fn current_style(styles: &[Style]) -> Style {
    styles.last().copied().unwrap_or_default()
}

fn owned_line(runs: Vec<StyledRun>) -> Line<'static> {
    Line::from(
        runs.into_iter()
            .map(|run| Span::styled(run.text, run.style))
            .collect::<Vec<_>>(),
    )
}

fn wrap_runs(runs: Vec<StyledRun>, width: usize) -> Vec<Vec<StyledRun>> {
    let mut out = vec![Vec::new()];
    let mut current_width = 0usize;
    for run in runs {
        for token in tokenize(&run.text) {
            if token == "\n" {
                out.push(Vec::new());
                current_width = 0;
                continue;
            }
            let token_width = UnicodeWidthStr::width(token.as_str());
            let is_space = token.trim().is_empty();
            if token_width > width && !is_space {
                if current_width > 0 {
                    out.push(Vec::new());
                    current_width = 0;
                }
                let mut chunk = String::new();
                let mut chunk_width = 0usize;
                for ch in token.chars() {
                    let ch_width = UnicodeWidthChar::width(ch).unwrap_or(0);
                    if chunk_width + ch_width > width && !chunk.is_empty() {
                        out.last_mut().expect("line exists").push(StyledRun {
                            text: std::mem::take(&mut chunk),
                            style: run.style,
                            link: run.link.clone(),
                        });
                        out.push(Vec::new());
                        chunk_width = 0;
                    }
                    chunk.push(ch);
                    chunk_width += ch_width;
                }
                if !chunk.is_empty() {
                    out.last_mut().expect("line exists").push(StyledRun {
                        text: chunk,
                        style: run.style,
                        link: run.link.clone(),
                    });
                    current_width = chunk_width;
                }
                continue;
            }
            if current_width + token_width > width && current_width > 0 && !is_space {
                out.push(Vec::new());
                current_width = 0;
            }
            if !(current_width == 0 && is_space) {
                out.last_mut().expect("line exists").push(StyledRun {
                    text: token,
                    style: run.style,
                    link: run.link.clone(),
                });
                current_width += token_width;
            }
        }
    }
    out
}

fn tokenize(text: &str) -> Vec<String> {
    let mut tokens = Vec::new();
    let mut current = String::new();
    let mut spacing = None;
    for ch in text.chars() {
        if ch == '\n' {
            if !current.is_empty() {
                tokens.push(std::mem::take(&mut current));
            }
            tokens.push("\n".to_string());
            spacing = None;
            continue;
        }
        let is_space = ch.is_whitespace();
        if spacing == Some(is_space) || spacing.is_none() {
            current.push(ch);
        } else {
            tokens.push(std::mem::take(&mut current));
            current.push(ch);
        }
        spacing = Some(is_space);
    }
    if !current.is_empty() {
        tokens.push(current);
    }
    tokens
}

fn append_link_targets(targets: &mut Vec<LinkTarget>, line: usize, runs: &[StyledRun]) {
    let mut column = 0;
    for run in runs {
        let width = UnicodeWidthStr::width(run.text.as_str());
        if let Some(target) = &run.link {
            if let Some(previous) = targets.last_mut().filter(|previous| {
                previous.line == line
                    && previous.target == *target
                    && previous.column + previous.width == column
            }) {
                previous.width += width;
            } else {
                targets.push(LinkTarget {
                    line,
                    column,
                    width,
                    target: target.clone(),
                });
            }
        }
        column += width;
    }
}

fn parse_heading(line: &str) -> Option<(usize, String)> {
    let trimmed = line.trim_start();
    let hashes = trimmed.chars().take_while(|c| *c == '#').count();
    (1..=6)
        .contains(&hashes)
        .then_some(trimmed[hashes..].trim_start())
        .filter(|text| !text.is_empty())
        .map(|text| (hashes, text.trim().trim_end_matches('#').trim().to_string()))
}

fn parse_fence_start(line: &str) -> Option<(&str, Option<String>)> {
    let trimmed = line.trim_start();
    trimmed.strip_prefix("```").map(|rest| {
        (
            "```",
            (!rest.trim().is_empty()).then_some(rest.trim().to_string()),
        )
    })
}

fn parse_image_line(line: &str) -> Option<(String, String)> {
    let trimmed = line.trim();
    let alt_start = trimmed.strip_prefix("![")?;
    let (alt, rest) = alt_start.split_once("](")?;
    let source = rest.strip_suffix(')')?;
    Some((alt.to_string(), source.to_string()))
}

fn parse_list_item(line: &str) -> Option<String> {
    let trimmed = line.trim_start();
    ["- ", "* ", "+ "]
        .iter()
        .find_map(|prefix| trimmed.strip_prefix(prefix))
        .map(ToString::to_string)
        .or_else(|| {
            let dot = trimmed.find(". ")?;
            trimmed[..dot]
                .chars()
                .all(|c| c.is_ascii_digit())
                .then(|| trimmed[dot + 2..].to_string())
        })
}

fn is_rule(line: &str) -> bool {
    let trimmed = line.trim();
    ["---", "***", "___"]
        .iter()
        .any(|marker| trimmed.len() >= 3 && trimmed.chars().all(|c| marker.contains(c)))
}

fn is_table_header(line: &str, next: Option<&str>) -> bool {
    line.contains('|')
        && next
            .map(|next| {
                let trimmed = next.trim().trim_matches('|').replace(' ', "");
                !trimmed.is_empty()
                    && trimmed
                        .split('|')
                        .all(|part| part.chars().all(|c| c == '-' || c == ':'))
            })
            .unwrap_or(false)
}

fn split_table_row(line: &str) -> Vec<String> {
    line.trim()
        .trim_matches('|')
        .split('|')
        .map(|part| part.trim().to_string())
        .collect()
}

fn enhanced_heading_tier(level: usize, downscale: HeadingDownscale) -> Option<KittyHeadingTier> {
    match (level, downscale) {
        (1, HeadingDownscale::None) => Some(KittyHeadingTier::H1),
        (1, HeadingDownscale::One) | (2, HeadingDownscale::None) => Some(KittyHeadingTier::H2),
        (1, HeadingDownscale::Two)
        | (2, HeadingDownscale::One | HeadingDownscale::Two)
        | (3, _) => Some(KittyHeadingTier::H3),
        _ => None,
    }
}

fn heading_style(level: usize) -> Style {
    let theme = crate::theme::active_theme();
    match level {
        1 => Style::default()
            .fg(theme.selection_fg)
            .bg(theme.warning)
            .add_modifier(Modifier::BOLD),
        2 => Style::default()
            .fg(theme.selection_fg)
            .bg(theme.info)
            .add_modifier(Modifier::BOLD),
        3 => Style::default()
            .fg(theme.selection_fg)
            .bg(theme.success)
            .add_modifier(Modifier::BOLD),
        4 => Style::default()
            .fg(theme.warning)
            .add_modifier(Modifier::BOLD),
        5 => Style::default()
            .fg(theme.accent_alt)
            .add_modifier(Modifier::BOLD),
        _ => Style::default()
            .fg(theme.muted)
            .add_modifier(Modifier::BOLD),
    }
}

fn heading_overlay_style(level: usize) -> Style {
    let theme = crate::theme::active_theme();
    match level {
        1 => Style::default()
            .fg(theme.warning)
            .bg(theme.background)
            .add_modifier(Modifier::BOLD),
        2 => Style::default()
            .fg(theme.info)
            .bg(theme.background)
            .add_modifier(Modifier::BOLD),
        _ => Style::default()
            .fg(theme.success)
            .bg(theme.background)
            .add_modifier(Modifier::BOLD),
    }
}
