use std::{cell::RefCell, collections::VecDeque, rc::Rc, time::Duration};

use merman_core::{
    diagrams::{
        pie::PieDiagramRenderModel,
        xychart::{XyChartAxisRenderModel, XyChartDiagramRenderModel, XyChartPlotType},
    },
    entities::decode_mermaid_entities_to_unicode,
    terminal_text::normalize_terminal_text,
    Engine, OperationControl, ParseOptions, RenderSemanticModel,
};
use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::{Color, Modifier, Style},
    symbols::Marker,
    text::{Line, Span},
    widgets::{
        canvas::{Canvas, Painter, Shape},
        Axis, Chart, Dataset, GraphType, Widget,
    },
};
use unicode_width::UnicodeWidthStr;

use super::{owned_line, wrap_runs, StyledRun};

const MAX_SOURCE_BYTES: usize = 8192;
const MAX_POINTS: usize = 256;
const MAX_SERIES: usize = 8;
const MAX_CATEGORIES: usize = 64;
const MAX_SLICES: usize = 16;

enum ChartModel {
    Xy(XyChartDiagramRenderModel),
    Pie(PieDiagramRenderModel),
}

type ParsedChart = Result<ChartModel, String>;

struct CachedChart {
    source: String,
    model: Rc<ParsedChart>,
}

thread_local! {
    // Cache semantic models, not colors: switching themes must recolor cached charts.
    static CHARTS: RefCell<VecDeque<CachedChart>> = const { RefCell::new(VecDeque::new()) };
}

pub(super) fn render_chart(
    lines: &mut Vec<Line<'static>>,
    source: &str,
    width: usize,
    closed: bool,
) -> bool {
    let kind = source
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty() && !line.starts_with("%%"))
        .and_then(|line| line.split_whitespace().next())
        .unwrap_or("");
    if !matches!(kind, "xychart-beta" | "xychart" | "pie") {
        return false;
    }
    let result = if !closed {
        Err("waiting for closing fence".into())
    } else if width < 20 {
        Err("chart needs at least 20 terminal columns".into())
    } else {
        let parsed = cached_chart(source);
        match parsed.as_ref() {
            Ok(ChartModel::Xy(model)) => render_xy(lines, model, width.min(160)),
            Ok(ChartModel::Pie(model)) => render_pie(lines, model, width.min(160)),
            Err(error) => Err(error.clone()),
        }
    };
    if let Err(error) = result {
        let theme = crate::theme::active_theme();
        append_text(
            lines,
            &format!("[Mermaid chart: {error}]"),
            width,
            theme.warning,
        );
        // Invalid/incomplete input stays inspectable, but cannot create unbounded output.
        let mut end = source.len().min(MAX_SOURCE_BYTES);
        while !source.is_char_boundary(end) {
            end -= 1;
        }
        for row in source[..end].lines().take(128) {
            append_text(lines, row, width, theme.code_fg);
        }
        if end < source.len() || source.lines().count() > 128 {
            append_text(
                lines,
                "[source preview truncated; copy retains original]",
                width,
                theme.muted,
            );
        }
    }
    true
}

fn cached_chart(source: &str) -> Rc<ParsedChart> {
    CHARTS.with(|charts| {
        let mut charts = charts.borrow_mut();
        if let Some(index) = charts.iter().position(|entry| entry.source == source) {
            if let Some(entry) = charts.remove(index) {
                let model = Rc::clone(&entry.model);
                charts.push_front(entry);
                return model;
            }
        }
        let model = Rc::new(
            std::panic::catch_unwind(|| parse_chart(source))
                .unwrap_or_else(|_| Err("parser rejected this chart".into())),
        );
        if source.len() <= MAX_SOURCE_BYTES {
            charts.truncate(15);
            charts.push_front(CachedChart {
                source: source.into(),
                model: Rc::clone(&model),
            });
        }
        model
    })
}

fn parse_chart(source: &str) -> ParsedChart {
    if source.len() > MAX_SOURCE_BYTES || source.lines().count() > 128 {
        return Err("chart exceeds rendering limits (8192 source bytes, 128 lines)".into());
    }
    let mut depth = 0usize;
    for ch in source.chars() {
        match ch {
            '[' | '(' | '{' => depth += 1,
            ']' | ')' | '}' => depth = depth.saturating_sub(1),
            _ => {}
        }
        if depth > 32 {
            return Err("chart exceeds nesting limit (32)".into());
        }
        if ch.is_control() && ch != '\n' && ch != '\r' && ch != '\t' {
            return Err("chart contains terminal control characters".into());
        }
    }
    let control = OperationControl::new().with_deadline(Duration::from_millis(150));
    let parsed = Engine::new()
        .parse_diagram_for_render_model_controlled_sync(source, ParseOptions::strict(), &control)
        .map_err(|error| error.to_string())?
        .map_err(|error| error.to_string())?
        .ok_or_else(|| "unsupported chart syntax".to_string())?;
    match parsed.model() {
        RenderSemanticModel::XyChart(model) => Ok(ChartModel::Xy(model.clone())),
        RenderSemanticModel::Pie(model) => Ok(ChartModel::Pie(model.clone())),
        _ => Err("unsupported chart syntax".into()),
    }
}

fn clean(text: &str) -> String {
    normalize_terminal_text(&decode_mermaid_entities_to_unicode(text))
        .chars()
        .filter(|ch| !ch.is_control())
        .collect()
}

fn append_text(lines: &mut Vec<Line<'static>>, text: &str, width: usize, color: Color) {
    let run = StyledRun {
        text: clean(text),
        style: Style::default().fg(color),
        link: None,
    };
    lines.extend(
        wrap_runs(vec![run], width.max(1))
            .into_iter()
            .map(owned_line),
    );
}

fn palette() -> [Color; 8] {
    let theme = crate::theme::active_theme();
    [
        theme.accent,
        theme.success,
        theme.warning,
        theme.info,
        theme.error,
        theme.accent_alt,
        theme.ansi[5],
        theme.ansi[6],
    ]
}

fn number(value: f64) -> String {
    if value == 0.0 {
        "0".into()
    } else if value.abs() >= 1_000_000.0 || value.abs() < 0.001 {
        format!("{value:.2e}")
    } else {
        format!("{value:.3}")
            .trim_end_matches('0')
            .trim_end_matches('.')
            .into()
    }
}

fn axis_title(axis: &XyChartAxisRenderModel) -> &str {
    match axis {
        XyChartAxisRenderModel::Band { title, .. }
        | XyChartAxisRenderModel::Linear { title, .. } => title,
    }
}

fn bounds(min: f64, max: f64) -> Result<[f64; 2], String> {
    if !min.is_finite() || !max.is_finite() || max < min || !(max - min).is_finite() {
        return Err("axes require finite, increasing numeric ranges".into());
    }
    if min == max {
        let padding = (min.abs() * 0.05).max(1.0);
        let range = [min - padding, max + padding];
        if !range[0].is_finite() || !range[1].is_finite() || range[0] == range[1] {
            return Err("axis magnitude exceeds rendering limits".into());
        }
        Ok(range)
    } else {
        Ok([min, max])
    }
}

fn render_xy(
    lines: &mut Vec<Line<'static>>,
    model: &XyChartDiagramRenderModel,
    width: usize,
) -> Result<(), String> {
    if model.orientation == "horizontal" {
        return Err("horizontal charts are not supported; use vertical xychart-beta".into());
    }
    let point_count: usize = model.plots.iter().map(|plot| plot.values.len()).sum();
    if model.plots.is_empty() || model.plots.iter().any(|plot| plot.values.is_empty()) {
        return Err("chart needs at least one nonempty bar or line series".into());
    }
    if model.plots.len() > MAX_SERIES || point_count > MAX_POINTS {
        return Err("chart exceeds rendering limits (8 series, 256 total points)".into());
    }
    let mut low = f64::INFINITY;
    let mut high = f64::NEG_INFINITY;
    for plot in &model.plots {
        for &value in &plot.values {
            if !value.is_finite() {
                return Err("chart values must be finite numbers".into());
            }
            low = low.min(value);
            high = high.max(value);
        }
    }
    let has_bars = model
        .plots
        .iter()
        .any(|plot| plot.plot_type == XyChartPlotType::Bar);
    if has_bars {
        low = low.min(0.0);
        high = high.max(0.0);
    }
    let XyChartAxisRenderModel::Linear { min, max, .. } = model.y_axis else {
        return Err("y-axis must be numeric".into());
    };
    if min.is_some_and(|value| !value.is_finite())
        || max.is_some_and(|value| !value.is_finite())
        || matches!((min, max), (Some(min), Some(max)) if max < min)
    {
        return Err("axes require finite, increasing numeric ranges".into());
    }
    // The semantic model supplies inferred ranges too. Include zero for bars and
    // all values so a valid series is never silently clipped from its chart.
    let y_bounds = bounds(min.unwrap_or(low).min(low), max.unwrap_or(high).max(high))?;
    let max_points = model
        .plots
        .iter()
        .map(|plot| plot.values.len())
        .max()
        .unwrap_or(1);
    let (x_bounds, categories) = match &model.x_axis {
        XyChartAxisRenderModel::Band { categories, .. } => {
            if categories.is_empty() || categories.len() > MAX_CATEGORIES {
                return Err("categorical charts require 1–64 categories".into());
            }
            if model
                .plots
                .iter()
                .any(|plot| plot.values.len() != categories.len())
            {
                return Err("each series must have one value per x-axis category".into());
            }
            (
                bounds(-0.5, categories.len() as f64 - 0.5)?,
                Some(categories),
            )
        }
        XyChartAxisRenderModel::Linear { min, max, .. } => (
            bounds(min.unwrap_or(1.0), max.unwrap_or(max_points as f64))?,
            None,
        ),
    };
    let colors = palette();
    let theme = crate::theme::active_theme();
    let mut points = Vec::with_capacity(model.plots.len());
    let bars = model
        .plots
        .iter()
        .filter(|plot| plot.plot_type == XyChartPlotType::Bar)
        .count();
    let plot_bounds = if bars > 0 && categories.is_none() {
        let padding =
            (x_bounds[1] - x_bounds[0]) / max_points.saturating_sub(1).max(1) as f64 / 2.0;
        bounds(x_bounds[0] - padding, x_bounds[1] + padding)?
    } else {
        x_bounds
    };
    let mut bar_index = 0;
    for plot in &model.plots {
        let count = plot.values.len();
        let step = if categories.is_some() {
            1.0
        } else if count > 1 {
            (x_bounds[1] - x_bounds[0]) / (count - 1) as f64
        } else {
            x_bounds[1] - x_bounds[0]
        };
        let mut data = Vec::new();
        for (index, &y) in plot.values.iter().enumerate() {
            let x = if categories.is_some() {
                index as f64
            } else if count == 1 {
                // Mermaid assigns a single numeric sample to the range minimum.
                match model.x_axis {
                    XyChartAxisRenderModel::Linear { min, .. } => min.unwrap_or(1.0),
                    _ => 0.0,
                }
            } else {
                x_bounds[0] + step * index as f64
            };
            if plot.plot_type == XyChartPlotType::Bar {
                if y == 0.0 {
                    continue;
                }
                let group_width = step * 0.7;
                let bar_width = group_width / bars.max(1) as f64;
                let center = x - group_width / 2.0 + bar_width * (bar_index as f64 + 0.5);
                let strokes = ((bar_width / (plot_bounds[1] - plot_bounds[0])) * width as f64)
                    .ceil()
                    .clamp(2.0, 160.0) as usize;
                for stroke in 0..strokes {
                    let offset = (stroke as f64 / (strokes - 1) as f64 - 0.5) * 0.9;
                    data.push((
                        (center + offset * bar_width).clamp(plot_bounds[0], plot_bounds[1]),
                        y,
                    ));
                }
            } else {
                data.push((x, y));
            }
        }
        if plot.plot_type == XyChartPlotType::Bar {
            bar_index += 1;
        }
        points.push(data);
    }
    let mut datasets = model
        .plots
        .iter()
        .zip(&points)
        .enumerate()
        .map(|(index, (plot, data))| {
            Dataset::default()
                .data(data)
                .style(Style::default().fg(colors[index]))
                .marker(if plot.plot_type == XyChartPlotType::Bar {
                    Marker::Block
                } else {
                    Marker::Braille
                })
                .graph_type(if plot.plot_type == XyChartPlotType::Bar {
                    GraphType::Bar
                } else {
                    GraphType::Line
                })
        })
        .collect::<Vec<_>>();
    let zero_baseline = [(plot_bounds[0], 0.0), (plot_bounds[1], 0.0)];
    if y_bounds[0] < 0.0 && y_bounds[1] > 0.0 {
        datasets.insert(
            0,
            Dataset::default()
                .data(&zero_baseline)
                .style(Style::default().fg(theme.muted))
                .marker(Marker::Braille)
                .graph_type(GraphType::Line),
        );
    }
    let x_labels = if categories.is_some() {
        // Reserve a label row; category ticks are centered on their actual samples below.
        vec![String::new(), String::new()]
    } else {
        vec![
            number(plot_bounds[0]),
            number(plot_bounds[0] + (plot_bounds[1] - plot_bounds[0]) / 2.0),
            number(plot_bounds[1]),
        ]
    };
    let y_labels = [
        y_bounds[0],
        y_bounds[0] + (y_bounds[1] - y_bounds[0]) / 2.0,
        y_bounds[1],
    ]
    .map(number);
    let height = (width / 4).clamp(10, 20) as u16;
    let area = Rect::new(0, 0, width as u16, height);
    let mut buffer = Buffer::empty(area);
    Chart::new(datasets)
        .x_axis(
            Axis::default()
                .bounds(plot_bounds)
                .labels(x_labels)
                .style(Style::default().fg(theme.muted)),
        )
        .y_axis(
            Axis::default()
                .bounds(y_bounds)
                .labels(y_labels)
                .style(Style::default().fg(theme.muted)),
        )
        .legend_position(None)
        .render(area, &mut buffer);
    if let Some(categories) = categories {
        let axis_row = height - 2;
        if let Some(axis_column) = (0..area.width).find(|&x| buffer[(x, axis_row)].symbol() == "└")
        {
            let left = axis_column + 1;
            let plot_width = area.width - left;
            let direct = categories.len() <= 3
                && categories
                    .iter()
                    .all(|label| UnicodeWidthStr::width(clean(label).as_str()) <= width / 6);
            let mut next_label = left;
            for (index, category) in categories.iter().enumerate() {
                let tick = left
                    + (((index as f64 + 0.5) / categories.len() as f64)
                        * plot_width.saturating_sub(1) as f64)
                        .round() as u16;
                let label = if direct {
                    clean(category)
                } else {
                    (index + 1).to_string()
                };
                let label_width = UnicodeWidthStr::width(label.as_str()) as u16;
                let start = tick.saturating_sub(label_width / 2).max(left);
                if start >= next_label && start + label_width <= area.width {
                    buffer[(tick, axis_row)].set_symbol("┬");
                    buffer.set_string(start, height - 1, &label, Style::default().fg(theme.muted));
                    next_label = start + label_width + 1;
                }
            }
        }
    }
    append_text(
        lines,
        model.title.as_deref().unwrap_or("XY chart"),
        width,
        theme.accent,
    );
    if !axis_title(&model.y_axis).is_empty() {
        append_text(
            lines,
            &format!("Y: {}", axis_title(&model.y_axis)),
            width,
            theme.foreground,
        );
    }
    append_buffer(lines, &buffer);
    if !axis_title(&model.x_axis).is_empty() {
        append_text(
            lines,
            &format!("X: {}", axis_title(&model.x_axis)),
            width,
            theme.foreground,
        );
    }
    for (index, plot) in model.plots.iter().enumerate() {
        let kind = if plot.plot_type == XyChartPlotType::Bar {
            "bar"
        } else {
            "line"
        };
        let default_name = format!("Series {}", index + 1);
        let name = plot.title.as_deref().unwrap_or(&default_name);
        append_text(
            lines,
            &format!("■ {}: {name} ({kind})", index + 1),
            width,
            colors[index],
        );
    }
    if let Some(categories) = categories {
        if categories.len() > 3
            || categories
                .iter()
                .any(|s| UnicodeWidthStr::width(clean(s).as_str()) > width / 6)
        {
            let mut row = String::new();
            let mut row_width = 0;
            for (index, label) in categories.iter().enumerate() {
                let entry = format!("{}: {}", index + 1, clean(label));
                let entry_width = UnicodeWidthStr::width(entry.as_str());
                if !row.is_empty() && row_width + 2 + entry_width > width {
                    append_text(lines, &row, width, theme.foreground);
                    row.clear();
                    row_width = 0;
                }
                if entry_width > width {
                    append_text(lines, &entry, width, theme.foreground);
                    continue;
                }
                if !row.is_empty() {
                    row.push_str("  ");
                    row_width += 2;
                }
                row.push_str(&entry);
                row_width += entry_width;
            }
            if !row.is_empty() {
                append_text(lines, &row, width, theme.foreground);
            }
        }
    }
    Ok(())
}

struct PieSectors<'a> {
    ends: &'a [f64],
    colors: &'a [Color; 8],
}

impl Shape for PieSectors<'_> {
    fn draw(&self, painter: &mut Painter) {
        let Some((right, bottom)) = painter.get_point(1.0, -1.0) else {
            return;
        };
        for row in 0..=bottom {
            for column in 0..=right {
                let x = 2.0 * column as f64 / right.max(1) as f64 - 1.0;
                let y = 1.0 - 2.0 * row as f64 / bottom.max(1) as f64;
                if x * x + y * y > 1.0 {
                    continue;
                }
                let angle = (x.atan2(y) / std::f64::consts::TAU).rem_euclid(1.0);
                let index = self
                    .ends
                    .iter()
                    .position(|end| angle < *end)
                    .unwrap_or(self.ends.len() - 1);
                painter.paint(column, row, self.colors[index % self.colors.len()]);
            }
        }
    }
}

fn render_pie(
    lines: &mut Vec<Line<'static>>,
    model: &PieDiagramRenderModel,
    width: usize,
) -> Result<(), String> {
    if model.sections.is_empty() || model.sections.len() > MAX_SLICES {
        return Err("pie requires 1–16 slices".into());
    }
    if model
        .sections
        .iter()
        .any(|section| !section.value.is_finite() || section.value < 0.0)
    {
        return Err("pie values must be finite and nonnegative".into());
    }
    let total: f64 = model.sections.iter().map(|section| section.value).sum();
    if !total.is_finite() || total <= 0.0 {
        return Err("pie total must be finite and greater than zero".into());
    }
    let mut cumulative = 0.0;
    let ends = model
        .sections
        .iter()
        .map(|section| {
            cumulative += section.value / total;
            cumulative
        })
        .collect::<Vec<_>>();
    let colors = palette();
    let diameter = width.min(40) as u16;
    let area = Rect::new(0, 0, diameter, diameter.div_ceil(2));
    let mut buffer = Buffer::empty(area);
    Canvas::default()
        .x_bounds([-1.0, 1.0])
        .y_bounds([-1.0, 1.0])
        .marker(Marker::HalfBlock)
        .paint(|context| {
            context.draw(&PieSectors {
                ends: &ends,
                colors: &colors,
            });
            let mut start = 0.0;
            for (index, &end) in ends.iter().enumerate() {
                if end - start >= 0.06 {
                    let angle = (start + (end - start) / 2.0) * std::f64::consts::TAU;
                    context.print(
                        0.65 * angle.sin(),
                        0.65 * angle.cos(),
                        Span::styled(
                            (index + 1).to_string(),
                            Style::default()
                                .fg(Color::Black)
                                .bg(colors[index % colors.len()]),
                        ),
                    );
                }
                start = end;
            }
        })
        .render(area, &mut buffer);
    let theme = crate::theme::active_theme();
    append_text(
        lines,
        model.title.as_deref().unwrap_or("Pie chart"),
        width,
        theme.accent,
    );
    append_buffer(lines, &buffer);
    for (index, section) in model.sections.iter().enumerate() {
        append_text(
            lines,
            &format!(
                "■ {}: {}  {} ({:.1}%)",
                index + 1,
                section.label,
                number(section.value),
                section.value / total * 100.0
            ),
            width,
            colors[index % colors.len()],
        );
    }
    append_text(
        lines,
        &format!("Total: {}", number(total)),
        width,
        theme.muted,
    );
    Ok(())
}

fn append_buffer(lines: &mut Vec<Line<'static>>, buffer: &Buffer) {
    for y in buffer.area.top()..buffer.area.bottom() {
        let mut spans = Vec::new();
        let mut current_style = None;
        let mut text = String::new();
        let mut x = buffer.area.left();
        while x < buffer.area.right() {
            let cell = &buffer[(x, y)];
            let symbol = cell.symbol();
            let style = Style::default()
                .fg(cell.fg)
                .bg(cell.bg)
                .add_modifier(cell.modifier & !Modifier::HIDDEN);
            if current_style.is_some_and(|previous| previous != style) {
                if let Some(previous) = current_style {
                    spans.push(Span::styled(std::mem::take(&mut text), previous));
                }
            }
            current_style = Some(style);
            text.push_str(symbol);
            x += UnicodeWidthStr::width(symbol).max(1) as u16;
        }
        if let Some(style) = current_style {
            spans.push(Span::styled(text, style));
        }
        lines.push(Line::from(spans));
    }
}
