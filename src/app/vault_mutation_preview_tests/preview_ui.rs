use std::path::{Path, PathBuf};

use ratatui::{backend::TestBackend, Terminal};

use crate::app::submission::{
    InputSubmission, PendingDocuments, PendingVaultMutation, PreparedUserTurn,
};
use crate::config::vault_profiles::{VaultCatalog, VaultProfile};
use crate::ui::modals::vault_mutation_preview::VaultMutationPreviewModal;
use crate::vault::move_plan::{MovePair, MovePlan, VaultFileRef};
use crate::vault::mutation_plan::VaultMutationKind;
use crate::vault::VaultRelativePath;

pub(super) struct VaultMutationFixture {
    pub(super) data_root: PathBuf,
    pub(super) source_root: PathBuf,
    pub(super) destination_root: PathBuf,
    pub(super) catalog: VaultCatalog,
    source: VaultProfile,
    destination: VaultProfile,
}

impl VaultMutationFixture {
    pub(super) fn new(label: &str) -> Self {
        let root = std::env::temp_dir().join(format!(
            "tcui-vault-mutation-preview-{label}-{}-{}",
            std::process::id(),
            rand::random::<u64>()
        ));
        let source_root = root.join("source");
        let destination_root = root.join("destination");
        let data_root = root.join("data");
        std::fs::create_dir_all(&source_root).expect("create source root");
        std::fs::create_dir_all(&destination_root).expect("create destination root");
        std::fs::create_dir_all(&data_root).expect("create data root");
        std::fs::write(source_root.join("source.md"), "source bytes").expect("write source");
        let source = VaultProfile::new("Work", &source_root).expect("source profile");
        let destination =
            VaultProfile::new("Archive", &destination_root).expect("destination profile");
        let mut catalog = VaultCatalog::default();
        catalog.add(source.clone()).expect("add source");
        catalog.add(destination.clone()).expect("add destination");
        Self {
            data_root,
            source_root,
            destination_root,
            catalog,
            source,
            destination,
        }
    }

    pub(super) fn create_plan(&self, overwrite: bool) -> MovePlan {
        let pair = MovePair::new(
            VaultFileRef::new(
                self.source.id.clone(),
                VaultRelativePath::parse(Path::new("source.md")).expect("source path"),
            ),
            VaultFileRef::new(
                self.destination.id.clone(),
                VaultRelativePath::parse(Path::new("moved.md")).expect("destination path"),
            ),
            overwrite,
        );
        MovePlan::build(vec![pair], &self.catalog, |_| false).expect("move plan")
    }

    fn pending(&self, overwrite: bool) -> PendingVaultMutation {
        let submission = InputSubmission::new(
            "@vault move source.md to Archive",
            PendingDocuments::default(),
        )
        .expect("submission");
        PendingVaultMutation::new(
            PreparedUserTurn::prepare(&submission),
            VaultMutationKind::Move(self.create_plan(overwrite)),
            &self.catalog,
        )
        .expect("pending preview")
    }
}

impl Drop for VaultMutationFixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(
            self.source_root
                .parent()
                .unwrap_or(self.source_root.as_path()),
        );
    }
}

#[test]
fn vault_mutation_preview_move_renders_exact_rows_totals_preconditions_and_controls() {
    let fixture = VaultMutationFixture::new("rows");
    let pending = fixture.pending(false);
    let backend = TestBackend::new(100, 24);
    let mut terminal = Terminal::new(backend).expect("terminal");

    terminal
        .draw(|frame| {
            VaultMutationPreviewModal::new(&pending).render(frame, frame.area());
        })
        .expect("draw preview");

    let rendered = terminal.backend().to_string();
    assert!(rendered.contains("Work:source.md -> Archive:moved.md"));
    assert!(rendered.contains("Total: 1 file, 12 bytes"));
    assert!(rendered.contains(&blake3::hash(b"source bytes").to_hex().to_string()));
    assert!(rendered.contains("create"));
    assert!(rendered.contains("[Cancel]"));
    assert!(rendered.contains("[Confirm]"));
}

#[test]
fn vault_mutation_preview_collision_disables_confirm() {
    let fixture = VaultMutationFixture::new("collision");
    std::fs::write(fixture.destination_root.join("moved.md"), "existing").expect("write collision");
    let pending = fixture.pending(false);

    assert!(!pending.preview.confirm_enabled);
    assert!(pending
        .preview
        .rows
        .iter()
        .any(|row| row.contains("unapproved collision")));
}

#[test]
fn vault_mutation_preview_scroll_and_tiny_terminal_render_safely() {
    let fixture = VaultMutationFixture::new("tiny");
    let mut pending = fixture.pending(false);
    pending.scroll_down(1);
    let backend = TestBackend::new(20, 6);
    let mut terminal = Terminal::new(backend).expect("terminal");

    terminal
        .draw(|frame| {
            VaultMutationPreviewModal::new(&pending).render(frame, frame.area());
        })
        .expect("draw tiny preview");

    assert_eq!(
        terminal.backend().buffer().area,
        ratatui::layout::Rect::new(0, 0, 20, 6)
    );
}
