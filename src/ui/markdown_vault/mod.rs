pub(crate) mod footer;
pub(crate) mod hit_areas;
pub(crate) mod layout;
mod move_input;
pub(crate) mod tree;

use ratatui::{prelude::*, widgets::Paragraph, Frame};

use crate::config::app_config::{HeadingDownscale, MarkdownMode};
use crate::ui::components::terminal_capabilities::TerminalCapabilities;
use crate::ui::markdown_vault_workspace::{
    MarkdownVaultFocus, MarkdownVaultWorkspaceState, MarkdownVaultWorkspaceStatus,
};
use crate::ui::modals::artifact_viewer::document_viewer::{
    DocumentViewerCapabilities, DocumentViewerProps,
};
use crate::ui::modals::quit_confirm::QuitConfirmModal;

use self::footer::{render_footer, MarkdownVaultFooterProps};
use self::hit_areas::MarkdownVaultMouseAction;
use self::layout::{split_content, workspace_layout};
use self::move_input::render_move_input;
use self::tree::render_tree;

#[derive(Debug, Clone, Copy)]
pub(crate) struct MarkdownVaultRenderProps<'a> {
    pub(crate) markdown_mode: MarkdownMode,
    pub(crate) kitty_enhanced_text: bool,
    pub(crate) kitty_heading_downscale: HeadingDownscale,
    pub(crate) image_protocol: &'a str,
    pub(crate) terminal_capabilities: TerminalCapabilities,
}

pub(crate) fn render_workspace(
    frame: &mut Frame,
    area: Rect,
    state: &mut MarkdownVaultWorkspaceState,
    props: MarkdownVaultRenderProps<'_>,
) {
    state.hit_areas.clear();
    let theme = crate::theme::active_theme();
    frame.render_widget(
        Paragraph::new("").style(Style::default().fg(theme.foreground).bg(theme.background)),
        area,
    );
    let layout = workspace_layout(area);
    render_tree(
        frame,
        layout.tree,
        &mut state.tree,
        state.focus == MarkdownVaultFocus::Tree,
        &mut state.hit_areas,
    );
    if layout.tree_divider.width > 0 {
        render_divider(frame, layout.tree_divider);
    }

    let content = split_content(layout.content, state.content_pane, state.editor.is_some());
    if let Some(viewer_area) = content.viewer {
        let viewer_block =
            Paragraph::new("").style(Style::default().fg(theme.foreground).bg(theme.background));
        frame.render_widget(viewer_block, viewer_area);
        state
            .hit_areas
            .push(viewer_area, MarkdownVaultMouseAction::FocusViewer);
        state.viewer.render(
            frame,
            viewer_area,
            DocumentViewerProps {
                markdown_mode: props.markdown_mode,
                kitty_enhanced_text: props.kitty_enhanced_text,
                kitty_heading_downscale: props.kitty_heading_downscale,
                image_protocol: props.image_protocol,
                terminal_capabilities: props.terminal_capabilities,
            },
            DocumentViewerCapabilities {
                edit: state
                    .tree
                    .selected_node()
                    .is_some_and(|node| node.is_editable()),
                ..DocumentViewerCapabilities::default()
            },
        );
        if let Some(edit) = state.viewer.hit_areas.edit {
            state
                .hit_areas
                .push(edit, MarkdownVaultMouseAction::EditSelected);
        }
    }
    if let Some(divider) = content.divider {
        render_divider(frame, divider);
    }
    if let (Some(editor_area), Some(editor)) = (content.editor, state.editor.as_mut()) {
        editor.render(frame, editor_area);
        state
            .hit_areas
            .push(editor_area, MarkdownVaultMouseAction::FocusEditor);
    }

    render_footer(
        frame,
        layout.footer,
        MarkdownVaultFooterProps {
            current_label: &state.profile_controls.current_label,
            expanded: state.profile_controls.expanded,
            selected_profile: state.profile_controls.selected,
            add_selected: state.profile_controls.add_selected,
            profiles: &state.profile_controls.profiles,
            move_available: state
                .tree
                .selected_node()
                .is_some_and(|node| node.entry_kind == crate::vault::VaultEntryKind::File),
        },
        &mut state.hit_areas,
    );

    if let Some(input) = state.move_input.as_ref() {
        render_move_input(frame, area, input);
    }

    let confirmation = match &state.status {
        MarkdownVaultWorkspaceStatus::RebindConfirmationRequired { profile_id } => Some((
            "Switch vault?",
            format!("Close the active editor and switch to vault {profile_id}?"),
        )),
        MarkdownVaultWorkspaceStatus::Ready
            if state.editor.as_ref().is_some_and(
                crate::ui::modals::terminal_editor::TerminalEditorState::close_is_pending,
            ) =>
        {
            Some((
                "Close editor?",
                "The editor may contain unsaved changes. Close it?".to_string(),
            ))
        }
        MarkdownVaultWorkspaceStatus::Ready => None,
    };
    if let Some((title, message)) = confirmation {
        let areas = QuitConfirmModal::new()
            .title(title)
            .message(&message)
            .render(frame);
        state
            .hit_areas
            .push(areas.yes, MarkdownVaultMouseAction::Confirm);
        state
            .hit_areas
            .push(areas.no, MarkdownVaultMouseAction::Cancel);
    }
}

fn render_divider(frame: &mut Frame, area: Rect) {
    let theme = crate::theme::active_theme();
    let lines = (0..area.height)
        .map(|_| Line::from("│"))
        .collect::<Vec<_>>();
    frame.render_widget(
        Paragraph::new(lines).style(Style::default().fg(theme.border).bg(theme.background)),
        area,
    );
}
