use ratatui::layout::Rect;

use crate::ui::markdown_vault_workspace::MarkdownVaultContentPane;

pub(crate) const CONTENT_SPLIT_MIN_WIDTH: u16 = 72;
const TREE_MIN_WIDTH: u16 = 20;
const TREE_MAX_WIDTH: u16 = 36;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct MarkdownVaultLayout {
    pub(crate) tree: Rect,
    pub(crate) tree_divider: Rect,
    pub(crate) content: Rect,
    pub(crate) footer: Rect,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct MarkdownVaultContentLayout {
    pub(crate) viewer: Option<Rect>,
    pub(crate) divider: Option<Rect>,
    pub(crate) editor: Option<Rect>,
}

pub(crate) fn workspace_layout(area: Rect) -> MarkdownVaultLayout {
    let footer_height = u16::from(area.height > 0);
    let body_height = area.height.saturating_sub(footer_height);
    let body = Rect::new(area.x, area.y, area.width, body_height);
    let proportional = area.width.saturating_mul(30) / 100;
    let tree_width = proportional
        .clamp(TREE_MIN_WIDTH, TREE_MAX_WIDTH)
        .min(body.width.saturating_sub(1));
    let divider_width = u16::from(body.width > tree_width);
    let tree = Rect::new(body.x, body.y, tree_width, body.height);
    let tree_divider = Rect::new(tree.right(), body.y, divider_width, body.height);
    let content = Rect::new(
        tree_divider.right(),
        body.y,
        body.width
            .saturating_sub(tree_width)
            .saturating_sub(divider_width),
        body.height,
    );
    let footer = Rect::new(
        area.x,
        area.bottom().saturating_sub(footer_height),
        area.width,
        footer_height,
    );
    MarkdownVaultLayout {
        tree,
        tree_divider,
        content,
        footer,
    }
}

pub(crate) fn split_content(
    area: Rect,
    focused: MarkdownVaultContentPane,
    editor_open: bool,
) -> MarkdownVaultContentLayout {
    if editor_open && area.width >= CONTENT_SPLIT_MIN_WIDTH {
        let left_width = area.width.saturating_sub(1) / 2;
        let divider = Rect::new(area.x.saturating_add(left_width), area.y, 1, area.height);
        let viewer = Rect::new(area.x, area.y, left_width, area.height);
        let editor = Rect::new(
            divider.right(),
            area.y,
            area.width.saturating_sub(left_width).saturating_sub(1),
            area.height,
        );
        return MarkdownVaultContentLayout {
            viewer: Some(viewer),
            divider: Some(divider),
            editor: Some(editor),
        };
    }
    match (focused, editor_open) {
        (MarkdownVaultContentPane::Editor, true) => MarkdownVaultContentLayout {
            editor: Some(area),
            ..MarkdownVaultContentLayout::default()
        },
        (MarkdownVaultContentPane::Viewer | MarkdownVaultContentPane::Editor, false)
        | (MarkdownVaultContentPane::Viewer, true) => MarkdownVaultContentLayout {
            viewer: Some(area),
            ..MarkdownVaultContentLayout::default()
        },
    }
}
