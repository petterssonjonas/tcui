use ratatui::layout::Rect;

use crate::ui::vault_workspace::VaultContentPane;

pub(crate) const CONTENT_SPLIT_MIN_WIDTH: u16 = 72;
const TREE_MIN_WIDTH: u16 = 20;
const TREE_MAX_WIDTH: u16 = 36;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct VaultLayout {
    pub(crate) tree: Rect,
    pub(crate) tree_divider: Rect,
    pub(crate) content: Rect,
    pub(crate) footer: Rect,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct VaultContentLayout {
    pub(crate) viewer: Option<Rect>,
    pub(crate) divider: Option<Rect>,
    pub(crate) editor: Option<Rect>,
}

pub(crate) fn workspace_layout(area: Rect) -> VaultLayout {
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
    VaultLayout {
        tree,
        tree_divider,
        content,
        footer,
    }
}

pub(crate) fn split_content(
    area: Rect,
    focused: VaultContentPane,
    editor_open: bool,
) -> VaultContentLayout {
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
        return VaultContentLayout {
            viewer: Some(viewer),
            divider: Some(divider),
            editor: Some(editor),
        };
    }
    match (focused, editor_open) {
        (VaultContentPane::Editor, true) => VaultContentLayout {
            editor: Some(area),
            ..VaultContentLayout::default()
        },
        (VaultContentPane::Viewer | VaultContentPane::Editor, false)
        | (VaultContentPane::Viewer, true) => VaultContentLayout {
            viewer: Some(area),
            ..VaultContentLayout::default()
        },
    }
}
