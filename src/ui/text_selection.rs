use ratatui::{
    buffer::{Buffer, CellDiffOption},
    layout::{Position, Rect},
    style::Modifier,
};
use unicode_width::UnicodeWidthStr;

#[derive(Default)]
pub struct TextSelection {
    screen: Buffer,
    start: Option<Position>,
    end: Option<Position>,
    bounds: Rect,
    dragged: bool,
}

impl TextSelection {
    pub fn is_active(&self) -> bool {
        self.start.is_some()
    }

    pub fn begin(&mut self, pos: Position, bounds: Rect) {
        self.cancel();
        self.bounds = bounds.intersection(self.screen.area);
        if self.bounds.contains(pos) {
            self.start = Some(pos);
            self.end = Some(pos);
        }
    }

    pub fn drag(&mut self, pos: Position) {
        if let Some(start) = self.start {
            let end = Position::new(
                pos.x
                    .clamp(self.bounds.x, self.bounds.right().saturating_sub(1)),
                pos.y
                    .clamp(self.bounds.y, self.bounds.bottom().saturating_sub(1)),
            );
            self.dragged |= end != start;
            self.end = Some(end);
        }
    }

    pub fn cancel(&mut self) {
        self.start = None;
        self.end = None;
        self.dragged = false;
    }

    pub fn finish(&mut self, pos: Position) -> Option<String> {
        self.drag(pos);
        let text = self.range().map(|(start, end)| {
            let mut text = String::new();
            for y in start.y..=end.y {
                let left = if y == start.y { start.x } else { self.bounds.x };
                let right = if y == end.y {
                    end.x + 1
                } else {
                    self.bounds.right()
                };
                let row_start = text.len();
                let mut x = self.bounds.x;
                while x < right {
                    let symbol = self.screen[(x, y)].symbol();
                    let width = UnicodeWidthStr::width(symbol).max(1) as u16;
                    if x.saturating_add(width) > left {
                        text.push_str(symbol);
                    }
                    x = x.saturating_add(width);
                }
                let trimmed = text[row_start..].trim_end_matches(' ').len();
                text.truncate(row_start + trimmed);
                if y != end.y {
                    text.push('\n');
                }
            }
            text
        });
        self.cancel();
        text
    }

    fn range(&self) -> Option<(Position, Position)> {
        if !self.dragged {
            return None;
        }
        let (start, end) = (self.start?, self.end?);
        Some(if (start.y, start.x) <= (end.y, end.x) {
            (start, end)
        } else {
            (end, start)
        })
    }

    pub fn render(&mut self, buffer: &mut Buffer) {
        if self.screen.area != buffer.area {
            self.cancel();
        }
        self.screen.area = buffer.area;
        self.screen.content.clone_from(&buffer.content);
        // Graphics protocols embed escape sequences in cell symbols. They are not
        // visible text, and Skip cells can retain stale text beneath an image.
        for cell in &mut self.screen.content {
            if cell.diff_option == CellDiffOption::Skip
                || cell.symbol().chars().any(char::is_control)
            {
                cell.set_symbol(" ");
            }
        }
        if let Some((start, end)) = self.range() {
            for y in start.y..=end.y {
                let left = if y == start.y { start.x } else { self.bounds.x };
                let right = if y == end.y {
                    end.x + 1
                } else {
                    self.bounds.right()
                };
                let mut x = self.bounds.x;
                while x < right {
                    let width = UnicodeWidthStr::width(self.screen[(x, y)].symbol()).max(1) as u16;
                    if x.saturating_add(width) > left {
                        for column in x..x.saturating_add(width).min(self.bounds.right()) {
                            buffer[(column, y)].modifier.insert(Modifier::REVERSED);
                        }
                    }
                    x = x.saturating_add(width);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::style::Style;

    #[test]
    fn graphics_protocols_and_hidden_cells_never_reach_clipboard() {
        let area = Rect::new(0, 0, 5, 1);
        let mut buffer = Buffer::empty(area);
        buffer.set_string(0, 0, "abcde", Style::default());
        buffer[(1, 0)].set_symbol("\x1b_Gf=24;AAAA\x1b\\");
        buffer[(2, 0)].set_symbol("\x1b]66;s=2;Heading\x1b\\");
        buffer[(3, 0)].set_diff_option(CellDiffOption::Skip);
        let mut selection = TextSelection::default();
        selection.render(&mut buffer);
        selection.begin(Position::new(0, 0), area);
        assert_eq!(
            selection.finish(Position::new(4, 0)).as_deref(),
            Some("a   e")
        );
    }

    #[test]
    fn reverse_drag_copies_multiline_unicode_without_wide_cell_padding() {
        let area = Rect::new(2, 3, 12, 2);
        let mut buffer = Buffer::empty(area);
        buffer.set_string(2, 3, "a界e\u{301} xyz", Style::default());
        buffer.set_string(2, 4, "last line", Style::default());
        let mut selection = TextSelection::default();
        selection.render(&mut buffer);
        selection.begin(Position::new(5, 4), area);
        selection.drag(Position::new(4, 3));
        selection.render(&mut buffer);
        assert!(buffer[(3, 3)].modifier.contains(Modifier::REVERSED));
        assert!(buffer[(4, 3)].modifier.contains(Modifier::REVERSED));
        assert_eq!(
            selection.finish(Position::new(4, 3)).as_deref(),
            Some("界e\u{301} xyz\nlast")
        );
    }

    #[test]
    fn click_does_not_copy_and_resize_cancels_drag() {
        let area = Rect::new(0, 0, 10, 2);
        let mut selection = TextSelection::default();
        selection.render(&mut Buffer::empty(area));
        selection.begin(Position::new(1, 0), area);
        assert_eq!(selection.finish(Position::new(1, 0)), None);
        selection.begin(Position::new(1, 0), area);
        selection.drag(Position::new(5, 0));
        selection.render(&mut Buffer::empty(Rect::new(0, 0, 5, 1)));
        assert_eq!(selection.finish(Position::new(3, 0)), None);
    }
}
