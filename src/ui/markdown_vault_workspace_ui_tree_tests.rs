use std::path::{Path, PathBuf};

use ratatui::layout::Rect;

use super::markdown_vault::layout::{split_content, workspace_layout};
use super::markdown_vault::tree::{build_tree_nodes, TreeSourceEntry};
use super::markdown_vault_workspace::MarkdownVaultContentPane;
use crate::ui::artifact_sidebar::ArtifactKind;
use crate::vault::VaultEntryKind;

#[test]
fn markdown_vault_workspace_ui_switches_split_exactly_at_72_content_columns() {
    let narrow = split_content(
        Rect::new(0, 0, 71, 10),
        MarkdownVaultContentPane::Editor,
        true,
    );
    let wide = split_content(
        Rect::new(0, 0, 72, 10),
        MarkdownVaultContentPane::Editor,
        true,
    );

    assert_eq!(narrow.editor, Some(Rect::new(0, 0, 71, 10)));
    assert!(narrow.viewer.is_none());
    assert!(narrow.divider.is_none());
    assert!(wide.viewer.is_some_and(|area| area.width >= 35));
    assert!(wide.editor.is_some_and(|area| area.width >= 35));
    assert_eq!(wide.divider.map(|area| area.width), Some(1));
}

#[test]
fn markdown_vault_workspace_ui_clamps_tree_to_20_and_36_columns() {
    assert_eq!(workspace_layout(Rect::new(0, 0, 64, 20)).tree.width, 20);
    assert_eq!(workspace_layout(Rect::new(0, 0, 200, 20)).tree.width, 36);
}

#[test]
fn markdown_vault_workspace_ui_reusable_tree_hides_dot_paths_and_collapses_directories() {
    let sources = vec![
        TreeSourceEntry::file(PathBuf::from("notes/a.md"), ArtifactKind::Markdown),
        TreeSourceEntry::file(PathBuf::from("notes/projects/b.md"), ArtifactKind::Markdown),
        TreeSourceEntry::file(
            PathBuf::from(".obsidian/plugins/demo/main.js"),
            ArtifactKind::Binary,
        ),
    ];

    let expanded = build_tree_nodes(&sources, &Default::default());
    let collapsed = build_tree_nodes(&sources, &std::iter::once(PathBuf::from("notes")).collect());

    assert!(expanded
        .iter()
        .any(|node| node.path == Path::new("notes/projects/b.md")));
    assert!(!expanded
        .iter()
        .any(|node| node.path.starts_with(".obsidian")));
    assert!(collapsed.iter().any(|node| node.path == Path::new("notes")));
    assert!(!collapsed
        .iter()
        .any(|node| node.path == Path::new("notes/a.md")));
}

#[test]
fn markdown_vault_workspace_ui_tree_source_preserves_typed_unsupported_entries() {
    let sources = [TreeSourceEntry::new(
        PathBuf::from("named-pipe"),
        VaultEntryKind::Unsupported,
        None,
    )];

    let nodes = build_tree_nodes(&sources, &Default::default());

    assert!(nodes[0].metadata().contains("Unsupported"));
    assert!(!nodes[0].is_editable());
}
