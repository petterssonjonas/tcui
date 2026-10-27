use std::borrow::Cow;

use merman_core::{
    diagrams::flowchart::{FlowEdgeMarker, FlowEdgeStroke, FlowEdgeVisibility, FlowchartModel},
    entities::decode_mermaid_entities_to_unicode,
    terminal_text::normalize_terminal_text,
    OperationControl,
};
use ratatui::{buffer::Buffer, layout::Rect, style::Style};
use unicode_width::UnicodeWidthStr;

use super::{plain, wrap_runs, DiagramResult};

const MAX_CELLS: usize = 65_536;
const MAX_BYTES: usize = 262_144;

pub(super) fn label_lines(text: &str, width: usize) -> Vec<String> {
    let mut text = Cow::Borrowed(text);
    for marker in ["<br>", "<br/>", "<br />", "\\n"] {
        if text.contains(marker) {
            text = Cow::Owned(text.replace(marker, "\n"));
        }
    }
    let text = decode_mermaid_entities_to_unicode(&text);
    let text = normalize_terminal_text(&text);
    text.split('\n')
        .flat_map(|line| {
            wrap_runs(vec![plain(line.into(), Style::default())], width.max(1))
                .into_iter()
                .map(|runs| runs.into_iter().map(|run| run.text).collect())
        })
        .collect()
}

#[derive(Clone, Copy)]
enum Item {
    Open(usize, usize),
    Node(usize, usize),
    Close(usize),
}

#[derive(Default)]
struct BoxLayout {
    x: usize,
    right: usize,
    top: usize,
    bottom: usize,
    labels: Vec<String>,
}

struct Route {
    edge: usize,
    from: usize,
    to: usize,
    source: usize,
    target: usize,
    lane: usize,
    labels: Vec<String>,
    label_top: usize,
}

fn limit_error() -> String {
    "diagram exceeds terminal routing limits".into()
}

// This is an orthogonal graph drawing, not a connectivity list: every edge has
// two distinct box ports, a continuous route, endpoint markers and inline text.
// A rank-based column layout handles ordinary branches first; the compact
// declaration-order router remains available for groups and narrow viewports.
pub(super) fn render(
    model: &FlowchartModel,
    width: usize,
    control: &OperationControl,
) -> DiagramResult {
    control.checkpoint().map_err(|error| error.to_string())?;
    if width < 12
        || width > u16::MAX as usize
        || model.nodes.len() > 64
        || model.edges.len() > 128
        || model.subgraphs.len() > 16
    {
        return Err(limit_error());
    }
    let nodes = model.nodes.len();
    let entities = nodes + model.subgraphs.len();
    let mut parents = vec![None; entities];
    let entity = |id: &str| {
        model
            .subgraphs
            .iter()
            .position(|group| group.id == id)
            .map(|index| nodes + index)
            .or_else(|| model.nodes.iter().position(|node| node.id == id))
    };
    for (group, subgraph) in model.subgraphs.iter().enumerate() {
        for member in &subgraph.nodes {
            if let Some(index) = entity(member) {
                if index == nodes + group {
                    return Err("diagram has cyclic group membership".into());
                }
                parents[index].get_or_insert(nodes + group);
            }
        }
    }
    for index in 0..entities {
        let mut cursor = parents[index];
        for depth in 0..=model.subgraphs.len() {
            let Some(parent) = cursor else { break };
            if depth == model.subgraphs.len() {
                return Err("diagram has cyclic group membership".into());
            }
            cursor = parents[parent];
        }
    }
    let mut items = Vec::with_capacity(entities + model.subgraphs.len());
    fn visit(
        parent: Option<usize>,
        depth: usize,
        nodes: usize,
        parents: &[Option<usize>],
        model: &FlowchartModel,
        items: &mut Vec<Item>,
    ) {
        for index in 0..parents.len() {
            if parents[index] != parent {
                continue;
            }
            if index < nodes {
                if !model.nodes[index].is_subgraph_anchor() {
                    items.push(Item::Node(index, depth));
                }
            } else {
                items.push(Item::Open(index, depth));
                visit(Some(index), depth + 1, nodes, parents, model, items);
                items.push(Item::Close(index));
            }
        }
    }
    visit(None, 0, nodes, &parents, model, &mut items);
    let mut ports = vec![Vec::new(); entities];
    let mut routes = Vec::with_capacity(model.edges.len());
    for (edge_index, edge) in model.edges.iter().enumerate() {
        if edge.visibility == FlowEdgeVisibility::Invisible {
            continue;
        }
        let (Some(from), Some(to)) = (entity(&edge.from), entity(&edge.to)) else {
            return Err("diagram has a connection without a node".into());
        };
        let index = routes.len();
        ports[from].push((index, true));
        ports[to].push((index, false));
        routes.push(Route {
            edge: edge_index,
            from,
            to,
            source: 0,
            target: 0,
            lane: 0,
            labels: Vec::new(),
            label_top: 0,
        });
    }
    // Bound text before either layout wraps it, including directly built models.
    let text_bytes = model
        .nodes
        .iter()
        .map(|node| node.label.as_deref().unwrap_or(&node.id).len())
        .chain(model.subgraphs.iter().map(|group| group.title.len()))
        .chain(
            model
                .edges
                .iter()
                .map(|edge| edge.label.as_deref().unwrap_or("").len()),
        )
        .try_fold(0usize, |sum, len| sum.checked_add(len))
        .ok_or_else(limit_error)?;
    if text_bytes > 16_384 {
        return Err(limit_error());
    }
    if model.subgraphs.is_empty() {
        if let Some(output) = render_columns(model, width, &ports, &routes, control)? {
            return Ok(output);
        }
    }
    // Port ordering does not depend on wrapped heights. Color intervals before
    // allocating label widths; disjoint routes reuse their columns.
    let mut rank = 0;
    let mut max_depth = 0;
    for item in &items {
        let (index, depth) = match *item {
            Item::Node(index, depth) | Item::Open(index, depth) => (index, depth),
            Item::Close(_) => continue,
        };
        max_depth = max_depth.max(depth);
        for &(route, source) in &ports[index] {
            if source {
                routes[route].source = rank;
            } else {
                routes[route].target = rank;
            }
            rank += 1;
        }
        rank += 1;
    }
    let mut order = (0..routes.len()).collect::<Vec<_>>();
    order.sort_unstable_by_key(|&index| (routes[index].source.min(routes[index].target), index));
    let mut ends = Vec::new();
    for index in order {
        let route = &mut routes[index];
        let start = route.source.min(route.target);
        let lane = match ends.iter().position(|&end| end < start) {
            Some(lane) => lane,
            None => {
                ends.push(0);
                ends.len() - 1
            }
        };
        ends[lane] = route.source.max(route.target);
        route.lane = lane;
    }
    let lane_width = ends.len().saturating_sub(1) * 2 + usize::from(!ends.is_empty());
    let frame_width = 4 * max_depth + 2;
    let Some(space) = width.checked_sub(lane_width + frame_width + 3) else {
        return Err(limit_error());
    };
    if space < 6 {
        return Err(limit_error());
    }
    let has_labels = routes.iter().any(|route| {
        model.edges[route.edge]
            .label
            .as_deref()
            .is_some_and(|s| !s.is_empty())
    });
    let label_width = if has_labels {
        (space / 2).clamp(2, 30)
    } else {
        0
    };
    let node_width = (space - label_width).clamp(4, 36);
    let outer_right = frame_width + node_width;
    let label_x = outer_right + 2;
    let lane_x = label_x + label_width + 1;
    let draw_width = lane_x + lane_width;
    if draw_width > width {
        return Err(limit_error());
    }
    for route in &mut routes {
        if let Some(label) = model.edges[route.edge]
            .label
            .as_deref()
            .filter(|label| !label.is_empty())
        {
            route.labels = label_lines(label, label_width.max(1));
        }
    }
    let mut boxes = (0..entities)
        .map(|_| BoxLayout::default())
        .collect::<Vec<_>>();
    let mut height = 0;
    for item in &items {
        control.checkpoint().map_err(|error| error.to_string())?;
        match *item {
            Item::Node(index, depth) | Item::Open(index, depth) => {
                let grouped = index >= nodes;
                let x = 2 * depth;
                let right = if grouped {
                    outer_right - 2 * depth
                } else {
                    2 * max_depth + node_width
                };
                if right < x + 3 {
                    return Err(limit_error());
                }
                let title = if grouped {
                    model.subgraphs[index - nodes].title.as_str()
                } else {
                    model.nodes[index]
                        .label
                        .as_deref()
                        .unwrap_or(&model.nodes[index].id)
                };
                let labels = label_lines(title, right - x - 3);
                let mut port_row = height + 1;
                for &(route_index, source) in &ports[index] {
                    let route = &mut routes[route_index];
                    let rows = if source { route.labels.len().max(1) } else { 1 };
                    if source {
                        route.label_top = port_row;
                        route.source = port_row + rows / 2;
                    } else {
                        route.target = port_row;
                    }
                    port_row += rows + 1;
                }
                let interior = labels.len().max(port_row.saturating_sub(height + 2)).max(1);
                boxes[index] = BoxLayout {
                    x,
                    right,
                    top: height,
                    bottom: height + interior + 1,
                    labels,
                };
                height += interior + 3;
            }
            Item::Close(index) => {
                boxes[index].bottom = height;
                height += 2;
            }
        }
        if height.saturating_mul(draw_width.max(1)) > MAX_CELLS {
            return Err(limit_error());
        }
    }
    height = height.saturating_sub(1);
    if height == 0 {
        return Ok(String::new());
    }
    let mut surface = Surface {
        buffer: Buffer::empty(Rect::new(0, 0, draw_width as u16, height as u16)),
    };
    // Draw outer groups first so nested boxes remain distinct.
    for item in &items {
        if let Item::Open(index, _) | Item::Node(index, _) = *item {
            surface.frame(&boxes[index]);
        }
    }
    for route in &routes {
        control.checkpoint().map_err(|error| error.to_string())?;
        let vertical = match model.edges[route.edge].stroke_kind {
            FlowEdgeStroke::Normal => "│",
            FlowEdgeStroke::Dotted => "┆",
            FlowEdgeStroke::Thick => "┃",
        };
        for y in route.source.min(route.target)..=route.source.max(route.target) {
            surface.glyph(lane_x + 2 * route.lane, y, vertical);
        }
    }
    for route in &routes {
        control.checkpoint().map_err(|error| error.to_string())?;
        let edge = &model.edges[route.edge];
        let x = lane_x + 2 * route.lane;
        let horizontal = match edge.stroke_kind {
            FlowEdgeStroke::Normal => "─",
            FlowEdgeStroke::Dotted => "┄",
            FlowEdgeStroke::Thick => "━",
        };
        for (y, other, endpoint, marker) in [
            (route.source, route.target, route.from, edge.start_marker),
            (route.target, route.source, route.to, edge.end_marker),
        ] {
            for column in boxes[endpoint].right + 1..x {
                let old = surface.buffer[(column as u16, y as u16)].symbol();
                // A bridge, not a junction: routes never share an endpoint port.
                let symbol = if ["│", "┆", "┃"].contains(&old) {
                    "╪"
                } else {
                    horizontal
                };
                surface.glyph(column, y, symbol);
            }
            surface.glyph(x, y, if y < other { "┐" } else { "┘" });
            surface.glyph(boxes[endpoint].right, y, "├");
            let marker = match marker {
                FlowEdgeMarker::None => horizontal,
                FlowEdgeMarker::Point => "◀",
                FlowEdgeMarker::Circle => "○",
                FlowEdgeMarker::Cross => "×",
            };
            surface.glyph(boxes[endpoint].right + 1, y, marker);
        }
        for (row, label) in route.labels.iter().enumerate() {
            if !label.is_empty() {
                let used = UnicodeWidthStr::width(label.as_str());
                surface.text(label_x, route.label_top + row, label);
                for column in used..label_width {
                    surface.glyph(label_x + column, route.label_top + row, " ");
                }
            }
        }
    }
    surface.output(control)
}

// Ignore DFS back edges only for ranking; every original edge is still routed.
// This keeps return arrows out of the forward hierarchy without collapsing a
// whole conversation cycle into one rank. All passes are bounded by 64/128.
fn graph_ranks(nodes: usize, routes: &[Route]) -> Vec<usize> {
    fn visit(node: usize, routes: &[Route], seen: &mut [bool], order: &mut Vec<usize>) {
        if seen[node] {
            return;
        }
        seen[node] = true;
        for route in routes.iter().filter(|route| route.from == node) {
            visit(route.to, routes, seen, order);
        }
        order.push(node);
    }
    let mut order = Vec::with_capacity(nodes);
    let mut seen = vec![false; nodes];
    for node in 0..nodes {
        visit(node, routes, &mut seen, &mut order);
    }
    order.reverse();
    let mut position = vec![0; nodes];
    for (index, &node) in order.iter().enumerate() {
        position[node] = index;
    }
    let mut ranks = vec![0; nodes];
    for node in order {
        for route in routes.iter().filter(|route| route.from == node) {
            if position[node] < position[route.to] {
                ranks[route.to] = ranks[route.to].max(ranks[node] + 1);
            }
        }
    }
    ranks
}

struct ColumnRoute {
    source: usize,
    target: usize,
    corridor: usize,
    source_lane: usize,
    target_lane: usize,
    label_top: usize,
    labels: Vec<String>,
}

fn render_columns(
    model: &FlowchartModel,
    width: usize,
    ports: &[Vec<(usize, bool)>],
    routes: &[Route],
    control: &OperationControl,
) -> Result<Option<String>, String> {
    let nodes = model.nodes.len();
    if nodes < 2 || model.nodes.iter().any(|node| node.is_subgraph_anchor()) {
        return Ok(None);
    }
    let ranks = graph_ranks(nodes, routes);
    let rank_count = ranks.iter().copied().max().unwrap_or(0) + 1;
    let direction = model.direction.as_deref().unwrap_or("TD");
    let reverse = matches!(direction, "BT" | "RL");
    let horizontal = matches!(direction, "LR" | "RL");
    let has_labels = routes.iter().any(|route| {
        model.edges[route.edge]
            .label
            .as_deref()
            .is_some_and(|label| !label.is_empty())
    });
    // At most three width candidates, never a search over cell positions.
    for capacity in (2..=(width / 32).min(4)).rev() {
        control.checkpoint().map_err(|error| error.to_string())?;
        let label_width = if has_labels { 12 } else { 0 };
        let mut positions = vec![(0usize, 0usize); nodes];
        let mut row_count = 0;
        if horizontal && rank_count <= capacity {
            let mut counts = vec![0; rank_count];
            for node in 0..nodes {
                let column = if reverse {
                    rank_count - 1 - ranks[node]
                } else {
                    ranks[node]
                };
                positions[node] = (column, counts[column]);
                counts[column] += 1;
                row_count = row_count.max(counts[column]);
            }
        } else {
            for layer in 0..rank_count {
                let rank = if reverse {
                    rank_count - 1 - layer
                } else {
                    layer
                };
                let mut count = 0;
                for node in 0..nodes {
                    if ranks[node] == rank {
                        positions[node] = (count % capacity, row_count + count / capacity);
                        count += 1;
                    }
                }
                row_count += count.div_ceil(capacity);
            }
        }
        let columns = positions
            .iter()
            .map(|&(column, _)| column)
            .max()
            .unwrap_or(0)
            + 1;
        if columns < 2 {
            continue;
        }
        let node_width = (width / columns)
            .saturating_sub(label_width + 10)
            .clamp(12, 26);
        let mut layouts = routes
            .iter()
            .map(|route| ColumnRoute {
                source: 0,
                target: 0,
                corridor: 0,
                source_lane: 0,
                target_lane: 0,
                label_top: 0,
                labels: model.edges[route.edge]
                    .label
                    .as_deref()
                    .filter(|label| !label.is_empty())
                    .map(|label| label_lines(label, label_width.max(1)))
                    .unwrap_or_default(),
            })
            .collect::<Vec<_>>();
        let mut boxes = Vec::with_capacity(nodes);
        let mut row_heights = vec![0usize; row_count];
        for node in 0..nodes {
            let labels = label_lines(
                model.nodes[node]
                    .label
                    .as_deref()
                    .unwrap_or(&model.nodes[node].id),
                node_width - 4,
            );
            let mut port = 1;
            for &(route, source) in &ports[node] {
                let layout = &mut layouts[route];
                if source {
                    layout.label_top = port;
                    layout.source = port + layout.labels.len().max(1) / 2;
                    port += layout.labels.len().max(1) + 1;
                } else {
                    layout.target = port;
                    port += 2;
                }
            }
            let height = (port.saturating_sub(1)).max(labels.len()).max(1) + 2;
            row_heights[positions[node].1] = row_heights[positions[node].1].max(height);
            boxes.push(BoxLayout {
                bottom: height - 1,
                labels,
                ..BoxLayout::default()
            });
        }
        let mut row_tops = vec![0; row_count];
        let mut height = 0usize;
        for row in 0..row_count {
            row_tops[row] = height;
            height += row_heights[row] + 1;
            for (index, route) in routes.iter().enumerate() {
                if positions[route.from].1 == row
                    && positions[route.from].0 != positions[route.to].0
                {
                    layouts[index].corridor = height;
                    height += 2;
                }
            }
        }
        for node in 0..nodes {
            let top = row_tops[positions[node].1];
            boxes[node].top = top;
            boxes[node].bottom += top;
            for &(route, source) in &ports[node] {
                if source {
                    layouts[route].source += top;
                    layouts[route].label_top += top;
                } else {
                    layouts[route].target += top;
                }
            }
        }
        let mut lane_counts = vec![0; columns];
        for (column, lane_count) in lane_counts.iter_mut().enumerate() {
            let mut intervals = Vec::new();
            for (index, route) in routes.iter().enumerate() {
                let layout = &layouts[index];
                let from = positions[route.from].0;
                let to = positions[route.to].0;
                if from == column {
                    let end = if from == to {
                        layout.target
                    } else {
                        layout.corridor
                    };
                    intervals.push((layout.source.min(end), layout.source.max(end), index, true));
                }
                if to == column && from != to {
                    intervals.push((
                        layout.target.min(layout.corridor),
                        layout.target.max(layout.corridor),
                        index,
                        false,
                    ));
                }
            }
            intervals.sort_unstable();
            let mut ends = Vec::new();
            for (start, end, index, source) in intervals {
                let lane = ends
                    .iter()
                    .position(|&previous| previous < start)
                    .unwrap_or(ends.len());
                if lane == ends.len() {
                    ends.push(end);
                } else {
                    ends[lane] = end;
                }
                if source {
                    layouts[index].source_lane = lane;
                    if positions[routes[index].from].0 == positions[routes[index].to].0 {
                        layouts[index].target_lane = lane;
                    }
                } else {
                    layouts[index].target_lane = lane;
                }
            }
            *lane_count = ends.len();
        }
        let mut column_x = vec![0; columns];
        let mut draw_width = 0;
        for column in 0..columns {
            column_x[column] = draw_width;
            draw_width += node_width
                + label_width
                + 3
                + lane_counts[column].saturating_sub(1) * 2
                + usize::from(lane_counts[column] != 0);
        }
        height = height.saturating_sub(1);
        if draw_width > width
            || height > u16::MAX as usize
            || height.saturating_mul(draw_width) > MAX_CELLS
        {
            continue;
        }
        for (node, frame) in boxes.iter_mut().enumerate() {
            frame.x = column_x[positions[node].0];
            frame.right = frame.x + node_width - 1;
        }
        let mut surface = Surface {
            buffer: Buffer::empty(Rect::new(0, 0, draw_width as u16, height as u16)),
        };
        for frame in &boxes {
            surface.frame(frame);
        }
        let paths = routes
            .iter()
            .zip(&layouts)
            .map(|(route, layout)| {
                let sx = boxes[route.from].right + label_width + 3 + 2 * layout.source_lane;
                let tx = boxes[route.to].right + label_width + 3 + 2 * layout.target_lane;
                let mut points = vec![
                    (boxes[route.from].right, layout.source),
                    (sx, layout.source),
                ];
                if positions[route.from].0 != positions[route.to].0 {
                    points.extend([(sx, layout.corridor), (tx, layout.corridor)]);
                }
                points.extend([(tx, layout.target), (boxes[route.to].right, layout.target)]);
                points
            })
            .collect::<Vec<_>>();
        // Vertical strokes first; horizontal crossings use bridges, not junctions.
        for vertical in [true, false] {
            for (index, path) in paths.iter().enumerate() {
                control.checkpoint().map_err(|error| error.to_string())?;
                let stroke = model.edges[routes[index].edge].stroke_kind;
                let (h, v) = match stroke {
                    FlowEdgeStroke::Normal => ("─", "│"),
                    FlowEdgeStroke::Dotted => ("┄", "┆"),
                    FlowEdgeStroke::Thick => ("━", "┃"),
                };
                for pair in path.windows(2) {
                    let [(x, y), (other_x, other_y)] = [pair[0], pair[1]];
                    if vertical && x == other_x {
                        for row in y.min(other_y)..=y.max(other_y) {
                            surface.glyph(x, row, v);
                        }
                    } else if !vertical && y == other_y {
                        for column in x.min(other_x)..=x.max(other_x) {
                            let old = surface.buffer[(column as u16, y as u16)].symbol();
                            let symbol = if ["│", "┆", "┃", "╪"].contains(&old) {
                                "╪"
                            } else {
                                h
                            };
                            surface.glyph(column, y, symbol);
                        }
                    }
                }
            }
        }
        for (index, path) in paths.iter().enumerate() {
            let route = &routes[index];
            let layout = &layouts[index];
            let edge = &model.edges[route.edge];
            for triple in path.windows(3) {
                let (x, y) = triple[1];
                let left = triple[0].0 < x || triple[2].0 < x;
                let down = triple[0].1 > y || triple[2].1 > y;
                surface.glyph(
                    x,
                    y,
                    match (left, down) {
                        (true, true) => "╮",
                        (true, false) => "╯",
                        (false, true) => "╭",
                        (false, false) => "╰",
                    },
                );
            }
            for (node, y, marker) in [
                (route.from, layout.source, edge.start_marker),
                (route.to, layout.target, edge.end_marker),
            ] {
                surface.glyph(boxes[node].right, y, "├");
                let marker = match marker {
                    FlowEdgeMarker::None => match edge.stroke_kind {
                        FlowEdgeStroke::Normal => "─",
                        FlowEdgeStroke::Dotted => "┄",
                        FlowEdgeStroke::Thick => "━",
                    },
                    FlowEdgeMarker::Point => "◀",
                    FlowEdgeMarker::Circle => "○",
                    FlowEdgeMarker::Cross => "×",
                };
                surface.glyph(boxes[node].right + 1, y, marker);
            }
            for (row, label) in layout.labels.iter().enumerate() {
                surface.text(boxes[route.from].right + 2, layout.label_top + row, label);
            }
        }
        return surface.output(control).map(Some);
    }
    Ok(None)
}

struct Surface {
    buffer: Buffer,
}

impl Surface {
    fn output(&self, control: &OperationControl) -> DiagramResult {
        let mut output = String::new();
        for y in 0..self.buffer.area.height {
            control.checkpoint().map_err(|error| error.to_string())?;
            let mut x = 0;
            while x < self.buffer.area.width {
                let symbol = self.buffer[(x, y)].symbol();
                output.push_str(symbol);
                x += UnicodeWidthStr::width(symbol).max(1) as u16;
            }
            output.truncate(output.trim_end_matches(' ').len());
            if y + 1 < self.buffer.area.height {
                output.push('\n');
            }
            if output.len() > MAX_BYTES {
                return Err(limit_error());
            }
        }
        Ok(output)
    }

    fn glyph(&mut self, x: usize, y: usize, symbol: &str) {
        self.buffer[(x as u16, y as u16)].set_symbol(symbol);
    }

    fn text(&mut self, x: usize, y: usize, text: &str) {
        self.buffer
            .set_string(x as u16, y as u16, text, Style::default());
    }

    fn frame(&mut self, frame: &BoxLayout) {
        for x in frame.x + 1..frame.right {
            self.glyph(x, frame.top, "─");
            self.glyph(x, frame.bottom, "─");
        }
        for y in frame.top + 1..frame.bottom {
            self.glyph(frame.x, y, "│");
            self.glyph(frame.right, y, "│");
        }
        self.glyph(frame.x, frame.top, "┌");
        self.glyph(frame.right, frame.top, "┐");
        self.glyph(frame.x, frame.bottom, "└");
        self.glyph(frame.right, frame.bottom, "┘");
        for (row, text) in frame.labels.iter().enumerate() {
            self.text(frame.x + 2, frame.top + 1 + row, text);
        }
    }
}
