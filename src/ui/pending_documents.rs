use crate::app::submission::{PendingDocument, PendingDocumentId};
use ratatui::{layout::Rect, style::Style, text::Line, widgets::Paragraph, Frame};
use std::sync::Arc;
use unicode_width::UnicodeWidthStr;

#[derive(Debug, Clone)]
pub struct PendingDocumentChip {
    id: PendingDocumentId,
    label: Arc<str>,
}

pub(crate) fn row_count(chips: &[PendingDocumentChip], width: u16) -> u16 {
    placements(chips, Rect::new(0, 0, width, u16::MAX))
        .last()
        .map_or(0, |placement| placement.area.y.saturating_add(1))
}

pub(crate) fn render(
    frame: &mut Frame,
    area: Rect,
    chips: &[PendingDocumentChip],
) -> Vec<(PendingDocumentId, Rect)> {
    let theme = crate::theme::active_theme();
    placements(chips, area)
        .into_iter()
        .map(|placement| {
            frame.render_widget(
                Paragraph::new(Line::from(placement.text))
                    .style(Style::default().fg(theme.foreground).bg(theme.user_bubble)),
                placement.area,
            );
            (placement.id, placement.close)
        })
        .collect()
}

struct ChipPlacement {
    id: PendingDocumentId,
    text: String,
    area: Rect,
    close: Rect,
}

fn placements(chips: &[PendingDocumentChip], area: Rect) -> Vec<ChipPlacement> {
    if area.width == 0 || area.height == 0 {
        return Vec::new();
    }
    let mut placements = Vec::with_capacity(chips.len());
    let mut x = area.x;
    let mut y = area.y;
    for chip in chips {
        let text = clipped_chip_text(chip, usize::from(area.width));
        let width = u16::try_from(UnicodeWidthStr::width(text.as_str()))
            .unwrap_or(area.width)
            .min(area.width);
        if x > area.x && x.saturating_add(width) > area.right() {
            x = area.x;
            y = y.saturating_add(1);
        }
        if y >= area.bottom() {
            break;
        }
        let rendered = Rect::new(x, y, width, 1);
        let close_width = width.min(3);
        let close = Rect::new(
            rendered.right().saturating_sub(close_width),
            y,
            close_width,
            1,
        );
        placements.push(ChipPlacement {
            id: chip.id(),
            text,
            area: rendered,
            close,
        });
        x = rendered.right().saturating_add(1);
    }
    placements
}

fn clipped_chip_text(chip: &PendingDocumentChip, width: usize) -> String {
    const CLOSE: &str = " [x]";
    let label = chip.label();
    if UnicodeWidthStr::width(label) + CLOSE.len() <= width {
        return format!("{label}{CLOSE}");
    }
    let available = width.saturating_sub(CLOSE.len());
    let mut clipped = label
        .chars()
        .take(available.saturating_sub(1))
        .collect::<String>();
    if available > 0 {
        clipped.push('~');
    }
    clipped.push_str(CLOSE);
    clipped
}

impl PendingDocumentChip {
    pub(crate) fn from_document(document: &PendingDocument) -> Self {
        Self {
            id: document.id(),
            label: if document.context_reference().is_some() {
                document.display_name_arc()
            } else {
                Arc::from(format!(
                    "[{} - {} lines]",
                    document.display_name(),
                    document.line_count()
                ))
            },
        }
    }

    pub const fn id(&self) -> PendingDocumentId {
        self.id
    }

    pub fn label(&self) -> &str {
        &self.label
    }
}
