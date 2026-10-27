use std::{borrow::Cow, cell::RefCell, collections::VecDeque, ops::Range, rc::Rc, time::Duration};

use merman_ascii::{
    AsciiError, AsciiLayoutProfile, AsciiRenderOptions, AsciiRenderer, AsciiResourceLimitId,
    AsciiResourcePolicy, AsciiViewportPolicy, OverflowPolicy,
};
use merman_core::{
    resources::ResourceProfile, Engine, OperationControl, ParseOptions, RenderSemanticModel,
};
use pulldown_cmark::{Alignment, Event, Options, Parser, Tag, TagEnd};
use ratatui::{
    style::{Modifier, Style},
    text::{Line, Span},
};
use unicode_width::UnicodeWidthStr;

use super::{
    append_link_targets, owned_line, parse_inline_events, wrap_runs, LinkTarget, StyledRun,
};

#[path = "markdown_flowchart.rs"]
mod flowchart;

#[derive(Debug)]
pub(super) struct Table {
    alignments: Vec<Alignment>,
    rows: Vec<Vec<Vec<StyledRun>>>,
}

pub(super) fn parse_tables(content: &str) -> VecDeque<(Range<usize>, Table)> {
    let content = protect_code_pipes(content);
    let masked_pipes = matches!(content, Cow::Owned(_));
    let mut tables = VecDeque::new();
    let mut table = None;
    let mut start = 0;
    let mut row = Vec::new();
    let mut cell = None;
    for (event, range) in Parser::new_ext(
        &content,
        Options::ENABLE_TABLES | Options::ENABLE_STRIKETHROUGH,
    )
    .into_offset_iter()
    {
        match event {
            Event::Start(Tag::Table(alignments)) => {
                start = range.start;
                table = Some(Table {
                    alignments,
                    rows: Vec::new(),
                });
            }
            Event::Start(Tag::TableHead | Tag::TableRow) if table.is_some() => row.clear(),
            Event::Start(Tag::TableCell) if table.is_some() => cell = Some(Vec::new()),
            Event::End(TagEnd::TableCell) => {
                if let Some(events) = cell.take() {
                    let style = if table.as_ref().is_some_and(|table| table.rows.is_empty()) {
                        Style::default().add_modifier(Modifier::BOLD)
                    } else {
                        Style::default()
                    };
                    let mut runs = parse_inline_events(events, style);
                    for run in &mut runs {
                        if masked_pipes {
                            run.text = run.text.replace('\u{1f}', "|");
                        }
                        run.text.retain(|ch| !ch.is_control() || ch == '\n');
                    }
                    row.push(runs);
                }
            }
            Event::End(TagEnd::TableHead | TagEnd::TableRow) => {
                if let Some(table) = &mut table {
                    table.rows.push(std::mem::take(&mut row));
                }
            }
            Event::End(TagEnd::Table) => {
                if let Some(table) = table.take() {
                    tables.push_back((start..range.end, table));
                }
            }
            event => {
                if let Some(events) = &mut cell {
                    events.push(event);
                }
            }
        }
    }
    tables
}

// GFM requires escaping pipes even in code spans. Also accept literal code pipes,
// using the Markdown parser's code-span ranges rather than splitting on raw pipes.
// The one-byte placeholder keeps source offsets valid for the outer block parser.
fn protect_code_pipes(content: &str) -> Cow<'_, str> {
    if !content.contains('|') || content.contains('\u{1f}') {
        return Cow::Borrowed(content);
    }
    let mut masked = None;
    let mut offset = 0;
    for line in content.split_inclusive('\n') {
        for (event, range) in Parser::new(line).into_offset_iter() {
            if matches!(event, Event::Code(_)) {
                for index in range {
                    if line.as_bytes()[index] == b'|' {
                        let escapes = line.as_bytes()[..index]
                            .iter()
                            .rev()
                            .take_while(|&&byte| byte == b'\\')
                            .count();
                        if escapes % 2 == 0 {
                            masked.get_or_insert_with(|| content.as_bytes().to_vec())
                                [offset + index] = 0x1f;
                        }
                    }
                }
            }
        }
        offset += line.len();
    }
    match masked.and_then(|bytes| String::from_utf8(bytes).ok()) {
        Some(masked) => Cow::Owned(masked),
        None => Cow::Borrowed(content),
    }
}

fn run_width(runs: &[StyledRun]) -> usize {
    runs.iter()
        .map(|run| UnicodeWidthStr::width(run.text.as_str()))
        .sum()
}

fn plain(text: String, style: Style) -> StyledRun {
    StyledRun {
        text,
        style,
        link: None,
    }
}

pub(super) fn render_table(
    lines: &mut Vec<Line<'static>>,
    targets: &mut Vec<LinkTarget>,
    table: Table,
    width: usize,
) {
    let width = width.max(1);
    let columns = table.alignments.len();
    if columns == 0 {
        return;
    }
    if width < 4 {
        // A wide grapheme needs two cells; a frame needs another two. Preserve content
        // without a frame when even one framed column cannot fit.
        for row in table.rows {
            for cell in row {
                for runs in wrap_runs(cell, width) {
                    append_link_targets(targets, lines.len(), &runs);
                    lines.push(owned_line(runs));
                }
            }
        }
        return;
    }
    let padding = usize::from(width >= 6);
    let panel_columns = ((width - 1) / (3 + 2 * padding)).max(1);
    let border_style = Style::default().fg(crate::theme::active_theme().border);
    for first in (0..columns).step_by(panel_columns) {
        let last = (first + panel_columns).min(columns);
        let count = last - first;
        let budget = width.saturating_sub(count + 1 + 2 * padding * count);
        let desired = (first..last)
            .map(|column| {
                table
                    .rows
                    .iter()
                    .filter_map(|row| row.get(column))
                    .map(|cell| {
                        cell.iter()
                            .flat_map(|run| run.text.lines())
                            .map(UnicodeWidthStr::width)
                            .sum::<usize>()
                    })
                    .max()
                    .unwrap_or(0)
                    .max(2)
            })
            .collect::<Vec<_>>();
        let mut widths = vec![2; count];
        for _ in 2 * count..budget {
            let Some(column) =
                (0..count).max_by_key(|&column| desired[column].saturating_sub(widths[column]))
            else {
                break;
            };
            if widths[column] >= desired[column] {
                break;
            }
            widths[column] += 1;
        }
        let border = |left: char, middle: char, right: char| {
            let pieces = widths
                .iter()
                .map(|width| "─".repeat(width + 2 * padding))
                .collect::<Vec<_>>();
            Line::from(Span::styled(
                format!("{left}{}{right}", pieces.join(&middle.to_string())),
                border_style,
            ))
        };
        lines.push(border('┌', '┬', '┐'));
        for (row_index, row) in table.rows.iter().enumerate() {
            let wrapped = (first..last)
                .enumerate()
                .map(|(index, column)| {
                    wrap_runs(row.get(column).cloned().unwrap_or_default(), widths[index])
                })
                .collect::<Vec<_>>();
            let height = wrapped.iter().map(Vec::len).max().unwrap_or(1);
            for line in 0..height {
                let mut runs = vec![plain("│".into(), border_style)];
                for (index, cell) in wrapped.iter().enumerate() {
                    let content = cell.get(line).cloned().unwrap_or_default();
                    let extra = widths[index].saturating_sub(run_width(&content));
                    let left = match table.alignments[first + index] {
                        Alignment::Right => extra,
                        Alignment::Center => extra / 2,
                        _ => 0,
                    };
                    runs.push(plain(" ".repeat(padding + left), Style::default()));
                    runs.extend(content);
                    runs.push(plain(" ".repeat(padding + extra - left), Style::default()));
                    runs.push(plain("│".into(), border_style));
                }
                append_link_targets(targets, lines.len(), &runs);
                lines.push(owned_line(runs));
            }
            if row_index + 1 < table.rows.len() {
                lines.push(border('├', '┼', '┤'));
            }
        }
        lines.push(border('└', '┴', '┘'));
    }
}

const MAX_SOURCE_BYTES: usize = 8192;
const CACHE_ENTRIES: usize = 32;
type DiagramResult = Result<String, String>;
struct CachedDiagram {
    source: String,
    width: usize,
    rendered: Rc<DiagramResult>,
}
thread_local! {
    // Cache both failures and successes: streamed/chat and viewer redraws must not
    // repeatedly parse or lay out an unchanged fenced diagram.
    static DIAGRAMS: RefCell<VecDeque<CachedDiagram>> = const { RefCell::new(VecDeque::new()) };
}

fn cached_diagram(source: &str, width: usize) -> Rc<DiagramResult> {
    DIAGRAMS.with(|cache| {
        let mut cache = cache.borrow_mut();
        if let Some(index) = cache
            .iter()
            .position(|entry| entry.source == source && entry.width == width)
        {
            if let Some(entry) = cache.remove(index) {
                let result = Rc::clone(&entry.rendered);
                cache.push_front(entry);
                return result;
            }
        }
        let rendered = Rc::new(
            std::panic::catch_unwind(|| diagram(source, width))
                .unwrap_or_else(|_| Err("renderer rejected this diagram".into())),
        );
        if source.len() <= MAX_SOURCE_BYTES {
            cache.truncate(CACHE_ENTRIES - 1);
            cache.push_front(CachedDiagram {
                source: source.into(),
                width,
                rendered: Rc::clone(&rendered),
            });
        }
        rendered
    })
}

fn diagram(source: &str, width: usize) -> DiagramResult {
    if source.len() > MAX_SOURCE_BYTES || source.lines().count() > 128 {
        return Err("diagram exceeds terminal rendering limits".into());
    }
    let mut depth = 0usize;
    for byte in source.bytes() {
        match byte {
            b'[' | b'(' | b'{' => depth += 1,
            b']' | b')' | b'}' => depth = depth.saturating_sub(1),
            _ => {}
        }
        if depth > 64 {
            return Err("diagram exceeds terminal nesting limit".into());
        }
    }
    let control = OperationControl::new().with_deadline(Duration::from_millis(150));
    let engine = Engine::new();
    let parsed = engine
        .parse_diagram_for_render_model_controlled_sync(source, ParseOptions::strict(), &control)
        .map_err(|error| error.to_string())?
        .map_err(|error| error.to_string())?
        .ok_or_else(|| {
            "unsupported diagram; terminal preview supports flowchart/graph".to_string()
        })?;
    let RenderSemanticModel::Flowchart(model) = parsed.model() else {
        return Err("unsupported diagram; terminal preview supports flowchart/graph".into());
    };
    if model.nodes.len() > 64 || model.edges.len() > 128 || model.subgraphs.len() > 16 {
        return Err(
            "diagram exceeds terminal graph limits (64 nodes, 128 edges, 16 subgraphs)".into(),
        );
    }
    if model
        .nodes
        .iter()
        .any(|node| node.link.is_some() || node.have_callback)
        || !model.tooltips.is_empty()
    {
        return Err("interactive links, callbacks and tooltips require source view".into());
    }
    if model
        .nodes
        .iter()
        .any(|node| node.label_type.as_deref() == Some("markdown"))
        || model
            .edges
            .iter()
            .any(|edge| edge.label_type.as_deref() == Some("markdown"))
    {
        return Err("Markdown Mermaid labels require source view".into());
    }
    let options = AsciiRenderOptions::unicode()
        .with_layout_profile(AsciiLayoutProfile::Auto)
        .with_flowchart_node_label_wrap_width(width.saturating_sub(6).clamp(2, 32));
    let renderer = AsciiRenderer::new(options).map_err(|error| error.to_string())?;
    let mut resources = AsciiResourcePolicy::for_profile(ResourceProfile::Constrained);
    for (id, maximum) in [
        (AsciiResourceLimitId::MaxGridCells, 65_536),
        (AsciiResourceLimitId::MaxDocumentCells, 65_536),
        (AsciiResourceLimitId::MaxOutputBytes, 262_144),
        (AsciiResourceLimitId::MaxLayoutWorkUnits, 250_000),
        (AsciiResourceLimitId::MaxNestingDepth, 32),
    ] {
        resources
            .apply_limit(id, maximum)
            .map_err(|error| error.to_string())?;
    }
    let context = engine
        .begin_operation()
        .map_err(|error| error.to_string())?;
    let rendered = match renderer.render_parsed_report(
        &parsed,
        AsciiViewportPolicy::with_max_width(width.max(1)).overflow(OverflowPolicy::Error),
        &control,
        &context,
        resources,
    ) {
        Ok(report) => report.text,
        // Native rank/route searches can exhaust their budget even for an ordinary
        // cyclic conversation graph. Reuse the parsed graph, not the source text,
        // with a separate deterministic, bounded orthogonal layout.
        Err(AsciiError::UnsupportedFeature { .. })
        | Err(AsciiError::WidthOverflow { .. })
        | Err(AsciiError::ResourceLimitExceeded(_))
        | Err(AsciiError::Cancelled(_)) => flowchart::render(
            model,
            width,
            &OperationControl::new().with_deadline(Duration::from_millis(150)),
        )?,
        Err(error) => return Err(error.to_string()),
    };
    // Defense in depth: dependency output is untrusted terminal content, not an
    // escape sequence transport. Keep only plain printable text and line breaks.
    if rendered.chars().any(|ch| ch.is_control() && ch != '\n') {
        return Err("diagram contains terminal control characters".into());
    }
    if rendered
        .lines()
        .any(|line| UnicodeWidthStr::width(line) > width)
    {
        return Err("diagram is wider than the available terminal; showing source".into());
    }
    Ok(rendered)
}

pub(super) fn render_mermaid(
    lines: &mut Vec<Line<'static>>,
    source: &str,
    width: usize,
    closed: bool,
) {
    let result = if closed {
        cached_diagram(source, width)
    } else {
        Rc::new(Err("waiting for closing fence".into()))
    };
    match result.as_ref() {
        Ok(text) => lines.extend(text.lines().map(|line| Line::from(line.to_owned()))),
        Err(reason) => {
            let heading = format!("[Mermaid source: {reason}]");
            for text in std::iter::once(heading.as_str()).chain(source.lines()) {
                let sanitized = text
                    .chars()
                    .filter(|ch| !ch.is_control())
                    .collect::<String>();
                let runs = vec![plain(
                    sanitized,
                    Style::default().fg(crate::theme::active_theme().code_fg),
                )];
                lines.extend(wrap_runs(runs, width.max(1)).into_iter().map(owned_line));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use merman_core::diagrams::flowchart::FlowchartModel;

    const SCREENSHOT_FLOWCHART: &str = include_str!("../../../evals/flowchart-return-labels.mmd");
    const CONVERSATION_FLOWCHART: &str = include_str!("../../../evals/flowchart-conversation.mmd");

    fn model(source: &str) -> FlowchartModel {
        let engine = Engine::new();
        let parsed = engine
            .parse_diagram_for_render_model_controlled_sync(
                source,
                ParseOptions::strict(),
                &OperationControl::new(),
            )
            .unwrap()
            .unwrap()
            .unwrap();
        match parsed.model() {
            RenderSemanticModel::Flowchart(model) => model.clone(),
            _ => panic!("expected flowchart"),
        }
    }

    fn letters(text: &str) -> String {
        text.chars().filter(|ch| ch.is_alphanumeric()).collect()
    }

    fn diagram_cells(output: &str) -> Vec<Vec<char>> {
        output
            .lines()
            .map(|line| {
                let mut cells = Vec::new();
                for ch in line.chars() {
                    cells.push(ch);
                    cells.extend(std::iter::repeat_n(
                        ' ',
                        unicode_width::UnicodeWidthChar::width(ch)
                            .unwrap_or(0)
                            .saturating_sub(1),
                    ));
                }
                cells
            })
            .collect()
    }

    fn diagram_boxes(rows: &[Vec<char>]) -> Vec<(usize, usize, usize, usize)> {
        let mut boxes = Vec::new();
        for (top, row) in rows.iter().enumerate() {
            for (left, &ch) in row.iter().enumerate() {
                if ch != '┌' {
                    continue;
                }
                let right = (left + 1..row.len()).find(|&x| row[x] == '┐').unwrap();
                let bottom = (top + 1..rows.len())
                    .find(|&y| rows[y].get(left) == Some(&'└'))
                    .unwrap();
                assert_eq!(rows[bottom].get(right), Some(&'┘'));
                boxes.push((left, top, right, bottom));
            }
        }
        boxes
    }

    fn box_text(rows: &[Vec<char>], frame: (usize, usize, usize, usize)) -> String {
        let (left, top, right, bottom) = frame;
        letters(
            &(top + 1..bottom)
                .flat_map(|y| rows[y][left + 1..right].iter())
                .collect::<String>(),
        )
    }

    fn regions(output: &str) -> (String, String) {
        let rows = diagram_cells(output);
        let frames = diagram_boxes(&rows);
        let boxes = frames.iter().map(|&frame| box_text(&rows, frame)).collect();
        let mut labels = String::new();
        for (y, row) in rows.iter().enumerate() {
            for (x, ch) in row.iter().enumerate() {
                if !frames.iter().any(|&(left, top, right, bottom)| {
                    x >= left && x <= right && y >= top && y <= bottom
                }) {
                    labels.push(*ch);
                }
            }
        }
        (boxes, letters(&labels))
    }

    fn fits(output: &str, width: usize) {
        assert!(
            output
                .lines()
                .all(|line| UnicodeWidthStr::width(line) <= width),
            "{output}"
        );
        assert!(
            !output.contains("Connections") && !output.contains("[1]"),
            "{output}"
        );
        assert!(!output.contains("Mermaid source"), "{output}");
        assert!(
            !output.chars().any(|ch| ch.is_control() && ch != '\n'),
            "{output}"
        );
    }

    #[test]
    fn screenshots_render_boxes_and_every_directional_connection_at_terminal_widths() {
        for (source, nodes, edges) in [
            (SCREENSHOT_FLOWCHART, 4, 6),
            (CONVERSATION_FLOWCHART, 11, 12),
        ] {
            for width in [140, 80, 40, 30, 24] {
                let output =
                    diagram(source, width).unwrap_or_else(|error| panic!("{width}: {error}"));
                fits(&output, width);
                assert_eq!(output.matches('┌').count(), nodes, "{width}: {output}");
                assert_eq!(
                    output
                        .chars()
                        .filter(|ch| "◀▶▲▼◄►↓↑←→".contains(*ch))
                        .count(),
                    edges,
                    "{width}: {output}",
                );
            }
        }
    }

    #[test]
    fn deterministic_layout_keeps_six_inline_labels_without_numbered_ports_or_key() {
        let model = model(SCREENSHOT_FLOWCHART);
        for width in [140, 80, 40, 30, 24] {
            let output = flowchart::render(&model, width, &OperationControl::new()).unwrap();
            fits(&output, width);
            assert_eq!(output.matches('┌').count(), 4, "{output}");
            assert_eq!(output.matches('◀').count(), 6, "{output}");
            assert_eq!(output.matches('├').count(), 12, "{output}");
            let (boxes, labels) = regions(&output);
            for node in &model.nodes {
                assert!(
                    boxes.contains(&letters(node.label.as_deref().unwrap_or(&node.id))),
                    "{output}"
                );
            }
            for edge in &model.edges {
                let label = edge.label.as_deref().unwrap();
                assert!(
                    labels.contains(&letters(label)),
                    "missing {label}: {output}"
                );
            }
        }
    }

    #[test]
    fn every_return_connection_can_be_traced_from_source_box_to_its_target_box() {
        for source in [SCREENSHOT_FLOWCHART, CONVERSATION_FLOWCHART] {
            for width in [140, 80, 40, 30, 24] {
                assert_connected_routes(source, width);
            }
        }
    }

    #[test]
    fn wide_conversation_places_branch_peers_beside_each_other_and_narrow_width_recovers() {
        let model = model(CONVERSATION_FLOWCHART);
        for width in [80, 140] {
            let output = flowchart::render(&model, width, &OperationControl::new()).unwrap();
            fits(&output, width);
            let rows = diagram_cells(&output);
            let frames = diagram_boxes(&rows);
            assert!(
                frames
                    .iter()
                    .any(|a| frames.iter().any(|b| { a.0 != b.0 && a.1 == b.1 })),
                "no side-by-side branch nodes at {width}: {output}"
            );
            let rendered = diagram(CONVERSATION_FLOWCHART, width).unwrap();
            let rendered_rows = diagram_cells(&rendered);
            let rendered_frames = diagram_boxes(&rendered_rows);
            assert!(
                rendered_frames
                    .iter()
                    .any(|a| rendered_frames.iter().any(|b| { a.0 != b.0 && a.1 == b.1 })),
                "recovery path lost columns at {width}: {rendered}"
            );
        }
        let narrow = flowchart::render(&model, 40, &OperationControl::new()).unwrap();
        fits(&narrow, 40);
        let rows = diagram_cells(&narrow);
        assert!(
            diagram_boxes(&rows).iter().all(|frame| frame.0 == 0),
            "{narrow}"
        );
    }

    #[test]
    fn columns_respect_rank_direction_and_keep_labeled_branches_and_return_routes() {
        for direction in ["TD", "BT", "LR", "RL"] {
            let source = format!("flowchart {direction}\nA[Start] -->|choose left| B[Left]\nA -->|choose right| C[Right]\nB -->|finish left| D[Done]\nC -->|finish right| D\nD -->|try again| A");
            let model = model(&source);
            let output = flowchart::render(&model, 140, &OperationControl::new()).unwrap();
            fits(&output, 140);
            let rows = diagram_cells(&output);
            let frames = diagram_boxes(&rows);
            let frame = |label: &str| {
                *frames
                    .iter()
                    .find(|&&frame| box_text(&rows, frame) == label)
                    .unwrap()
            };
            let start = frame("Start");
            let done = frame("Done");
            match direction {
                "LR" => assert!(start.0 < done.0, "{output}"),
                "RL" => assert!(start.0 > done.0, "{output}"),
                "BT" => assert!(start.1 > done.1, "{output}"),
                _ => assert!(start.1 < done.1, "{output}"),
            }
            assert_connected_routes(&source, 140);
            for edge in &model.edges {
                let from = model
                    .nodes
                    .iter()
                    .find(|node| node.id == edge.from)
                    .unwrap();
                let source_frame = frame(from.label.as_deref().unwrap_or(&from.id));
                let label = letters(edge.label.as_deref().unwrap());
                let inline = (source_frame.1 + 1..source_frame.3)
                    .flat_map(|y| rows[y].iter().skip(source_frame.2 + 2).take(12))
                    .collect::<String>();
                assert!(
                    letters(&inline).contains(&label),
                    "lost inline label {label}: {output}"
                );
            }
        }
    }

    fn assert_connected_routes(source: &str, width: usize) {
        let model = model(source);
        let output = flowchart::render(&model, width, &OperationControl::new()).unwrap();
        let rows = diagram_cells(&output);
        let frames = diagram_boxes(&rows);
        let node_at = |x: usize, y: usize| {
            let frame = frames
                .iter()
                .find(|&&(_, top, right, bottom)| x == right && y > top && y < bottom)
                .unwrap();
            let text = box_text(&rows, *frame);
            model
                .nodes
                .iter()
                .position(|node| {
                    text == letters(
                        &flowchart::label_lines(node.label.as_deref().unwrap_or(&node.id), 1024)
                            .join(""),
                    )
                })
                .unwrap()
        };
        let mut connections = Vec::new();
        for (y, row) in rows.iter().enumerate() {
            for (port, &ch) in row.iter().enumerate() {
                if ch != '├' || row.get(port + 1) == Some(&'◀') {
                    continue;
                }
                let source = node_at(port, y);
                let (mut x, mut target_y) = (port as isize + 1, y as isize);
                let (mut dx, mut dy) = (1isize, 0isize);
                let mut outgoing = true;
                let mut target = None;
                for _ in 0..65_536 {
                    assert!(x >= 0 && target_y >= 0, "{output}");
                    let cell = rows
                        .get(target_y as usize)
                        .and_then(|row| row.get(x as usize))
                        .copied()
                        .unwrap_or(' ');
                    if cell == '├' {
                        assert_eq!((dx, dy), (-1, 0), "{output}");
                        assert_eq!(rows[target_y as usize][x as usize + 1], '◀', "{output}");
                        target = Some(node_at(x as usize, target_y as usize));
                        break;
                    }
                    let corner = match cell {
                        '┐' | '╮' => Some([(-1, 0), (0, 1)]),
                        '┘' | '╯' => Some([(-1, 0), (0, -1)]),
                        '╭' => Some([(1, 0), (0, 1)]),
                        '╰' => Some([(1, 0), (0, -1)]),
                        _ => None,
                    };
                    if let Some(sides) = corner {
                        let incoming = (-dx, -dy);
                        assert!(
                            sides.contains(&incoming),
                            "wrong turn at {x},{target_y}: {output}"
                        );
                        (dx, dy) = *sides.iter().find(|&&side| side != incoming).unwrap();
                        outgoing = false;
                    } else if dy != 0 {
                        assert!(
                            "│┆┃╪".contains(cell),
                            "broken vertical at {x},{target_y}: {output}"
                        );
                    } else if !outgoing || model.edges.iter().all(|edge| edge.label.is_none()) {
                        assert!(
                            "─┄━╪◀".contains(cell),
                            "broken horizontal at {x},{target_y}: {output}"
                        );
                    }
                    x += dx;
                    target_y += dy;
                }
                connections.push((
                    source,
                    target.unwrap_or_else(|| panic!("unterminated route: {output}")),
                ));
            }
        }
        let mut expected = model
            .edges
            .iter()
            .map(|edge| {
                (
                    model
                        .nodes
                        .iter()
                        .position(|node| node.id == edge.from)
                        .unwrap(),
                    model
                        .nodes
                        .iter()
                        .position(|node| node.id == edge.to)
                        .unwrap(),
                )
            })
            .collect::<Vec<_>>();
        connections.sort_unstable();
        expected.sort_unstable();
        assert_eq!(connections, expected, "{output}");
    }

    #[test]
    fn deterministic_layout_preserves_cycles_parallel_bidirectional_and_detached_nodes() {
        for direction in ["TD", "BT", "LR", "RL"] {
            let mut source = format!("flowchart {direction}\nA[世界🙂<br>Client] <--> B[Service]\nA -->|loop| A\nZ[Detached]\n");
            for index in 0..10 {
                source.push_str(&format!("B -->|response {index}| A\n"));
            }
            let model = model(&source);
            let output = flowchart::render(&model, 100, &OperationControl::new()).unwrap();
            fits(&output, 100);
            assert_eq!(output.matches('┌').count(), 3, "{output}");
            assert_eq!(output.matches('◀').count(), 13, "{output}");
            assert_eq!(output.matches('├').count(), 24, "{output}");
            assert!(
                output.contains("世界🙂") && output.contains("Detached"),
                "{output}"
            );
            let (_, labels) = regions(&output);
            assert!(labels.contains("loop"), "{output}");
            for index in 0..10 {
                assert!(labels.contains(&format!("response{index}")), "{output}");
            }
        }
    }

    #[test]
    fn deterministic_layout_draws_nested_group_frames_and_group_endpoint_arrows() {
        let source = "flowchart TD\nsubgraph Outer[Outer group]\nsubgraph Inner[Inner group]\nA[Read] --> B[Write]\nend\nC[Check]\nB --> C\nend\nOuter --> D[Done]\nD --> A\nC --> A";
        let model = model(source);
        for width in [100, 60, 40] {
            let output = flowchart::render(&model, width, &OperationControl::new()).unwrap();
            fits(&output, width);
            assert_eq!(output.matches('┌').count(), 6, "{output}");
            assert_eq!(output.matches('◀').count(), 5, "{output}");
            for label in [
                "Outer group",
                "Inner group",
                "Read",
                "Write",
                "Check",
                "Done",
            ] {
                assert!(output.contains(label), "{output}");
            }
        }
    }

    #[test]
    fn deterministic_layout_normalizes_labels_and_preserves_bounded_failure() {
        let mut model = model("flowchart TD\nA[Client] --> B[Service]\nB --> A");
        model.nodes[0].label = Some("世界🙂<br>Next &amp; final\u{1b}".into());
        model.edges[0].label = Some("send\\nreply &amp; finish\u{7}".into());
        let output = flowchart::render(&model, 80, &OperationControl::new()).unwrap();
        fits(&output, 80);
        assert!(
            output.contains("世界🙂") && output.contains("Next & final"),
            "{output}"
        );
        assert!(regions(&output).1.contains("sendreplyfinish"), "{output}");
        assert!(flowchart::render(&model, 8, &OperationControl::new()).is_err());
        let control = OperationControl::new();
        control.cancel();
        assert!(flowchart::render(&model, 80, &control).is_err());
        model.nodes[0].label = Some("long ".repeat(20_000));
        assert!(flowchart::render(&model, 80, &OperationControl::new()).is_err());
    }

    #[test]
    fn deterministic_layout_keeps_unicode_inline_labels_strokes_markers_and_invisible_edges() {
        use merman_core::diagrams::flowchart::{
            FlowEdgeMarker, FlowEdgeStroke, FlowEdgeVisibility,
        };
        let mut model = model("flowchart TD\nA[Client] --> B[Service]\nB --> C[Storage]\nC --> A");
        model.edges[0].label = Some("Unicode 世界🙂<br>Next &amp; final".into());
        model.edges[0].start_marker = FlowEdgeMarker::Circle;
        model.edges[0].end_marker = FlowEdgeMarker::Cross;
        model.edges[0].stroke_kind = FlowEdgeStroke::Dotted;
        model.edges[1].stroke_kind = FlowEdgeStroke::Thick;
        model.edges[2].visibility = FlowEdgeVisibility::Invisible;
        let output = flowchart::render(&model, 80, &OperationControl::new()).unwrap();
        fits(&output, 80);
        assert!(output.contains("世界🙂"), "{output}");
        assert!(
            regions(&output).1.contains("Unicode世界Nextfinal"),
            "{output}"
        );
        for symbol in ['○', '×', '┄', '┆', '━', '┃'] {
            assert!(output.contains(symbol), "missing {symbol}: {output}");
        }
        assert_eq!(output.matches('├').count(), 4, "{output}");
        assert_eq!(output.matches('◀').count(), 1, "{output}");
    }

    #[test]
    fn repeated_diagram_redraws_reuse_layout_including_errors() {
        let first = cached_diagram("flowchart TD\nA[Cached] --> B[Output]", 80);
        let second = cached_diagram("flowchart TD\nA[Cached] --> B[Output]", 80);
        assert!(Rc::ptr_eq(&first, &second));
        let first = cached_diagram("not Mermaid", 80);
        let second = cached_diagram("not Mermaid", 80);
        assert!(Rc::ptr_eq(&first, &second));
    }
}
