use std::collections::HashSet;

use super::TuiApp;

impl TuiApp {
    pub(crate) fn refresh_vault_artifacts(&mut self) {
        self.ui.vault_artifacts = self
            .vault
            .as_ref()
            .and_then(|vault| {
                vault.list_files(None).ok().map(|paths| {
                    paths
                        .into_iter()
                        .map(|path| {
                            crate::ui::artifact_sidebar::ArtifactEntry::vault_file(
                                &vault.root,
                                &path,
                            )
                        })
                        .collect()
                })
            })
            .unwrap_or_default();
    }

    pub(crate) fn refresh_saved_artifacts(&mut self) {
        let root = self.export_base_dir();
        self.ui.saved_artifacts = std::fs::read_dir(&root)
            .ok()
            .into_iter()
            .flat_map(|entries| entries.filter_map(Result::ok))
            .map(|entry| entry.path())
            .filter(|path| path.is_file())
            .map(crate::ui::artifact_sidebar::ArtifactEntry::saved_file)
            .collect();
        self.ui
            .saved_artifacts
            .sort_by(|left, right| left.name.cmp(&right.name));
    }

    pub(crate) fn refresh_memory_artifacts(&mut self) {
        self.ui.memory_artifacts.clear();
        #[cfg(feature = "memory")]
        {
            let Some(vault) = self.vault.as_ref() else {
                return;
            };
            let Ok(store) = crate::memory::MemoryStore::open(
                &vault.root,
                &crate::memory::MemoryStore::default_cache_path(),
            ) else {
                return;
            };
            let Ok(documents) = store.active_documents() else {
                return;
            };
            self.ui.memory_artifacts = documents
                .into_iter()
                .map(|(path, document)| {
                    let body = strip_frontmatter(&document.markdown);
                    crate::ui::artifact_sidebar::ArtifactEntry::memory_file(
                        document.logical_path,
                        document.title,
                        body.to_string(),
                        path,
                    )
                })
                .collect();
        }
    }

    pub(crate) fn refresh_artifact_sidebar_catalogs(&mut self) {
        self.refresh_vault_artifacts();
        self.refresh_saved_artifacts();
        self.refresh_memory_artifacts();
    }

    pub(crate) fn prepare_artifact_save(
        &mut self,
        handle: crate::ui::artifact_sidebar::ArtifactHandle,
    ) {
        let Some(artifact) = self.find_artifact(&handle) else {
            return;
        };
        if matches!(
            artifact.origin,
            crate::ui::artifact_sidebar::ArtifactOrigin::Saved
        ) {
            let base_dir = self
                .config
                .try_read()
                .ok()
                .and_then(|cfg| cfg.artifact_save_dir.clone())
                .map(|path| {
                    crate::app::generated_file::expand_user_path(
                        std::path::Path::new(&path),
                        dirs::home_dir().as_deref(),
                    )
                })
                .or_else(dirs::download_dir)
                .or_else(|| std::env::current_dir().ok())
                .unwrap_or_else(|| std::path::PathBuf::from("."));
            self.ui.save_file_dialog = Some(crate::ui::modals::save_file::SaveFileDialog::new(
                &artifact, base_dir, "Export",
            ));
            return;
        }
        if artifact.is_markdown() && self.vault.is_some() {
            if self.save_temp_artifact_to_vault(&artifact).is_ok() {
                self.ui.connection_status = crate::ui::status_bar::ConnectionStatus::CloudConnected;
                self.ui.connection_message = Some(format!("Saved {} to vault", artifact.name));
            }
            return;
        }

        if matches!(
            artifact.origin,
            crate::ui::artifact_sidebar::ArtifactOrigin::Memory
        ) {
            if let crate::ui::artifact_sidebar::ArtifactHandle::Memory(path) = artifact.handle {
                self.open_memory_export_dialog(path);
            }
            return;
        }

        let base_dir = self
            .config
            .try_read()
            .ok()
            .and_then(|cfg| cfg.artifact_save_dir.clone())
            .map(|path| {
                crate::app::generated_file::expand_user_path(
                    std::path::Path::new(&path),
                    dirs::home_dir().as_deref(),
                )
            })
            .or_else(dirs::download_dir)
            .or_else(|| std::env::current_dir().ok())
            .unwrap_or_else(|| std::path::PathBuf::from("."));
        self.ui.save_file_dialog = Some(crate::ui::modals::save_file::SaveFileDialog::new(
            &artifact, base_dir, "Save",
        ));
    }

    pub(crate) fn save_temp_artifact_to_vault(
        &mut self,
        artifact: &crate::ui::artifact_sidebar::ArtifactEntry,
    ) -> color_eyre::Result<()> {
        let Some(vault) = &self.vault else {
            return Ok(());
        };
        let content = artifact.content.clone().unwrap_or_default();
        vault.write_file(std::path::Path::new(&artifact.name), &content)?;
        self.remove_temporary_artifact(&artifact.handle);
        self.refresh_artifact_sidebar_catalogs();
        Ok(())
    }

    pub(crate) fn promote_temporary_artifact_if_vault_path(
        &mut self,
        handle: &crate::ui::artifact_sidebar::ArtifactHandle,
        saved_path: &std::path::Path,
    ) {
        let Some(vault) = &self.vault else {
            return;
        };
        if saved_path.starts_with(&vault.root) {
            self.remove_temporary_artifact(handle);
            self.refresh_artifact_sidebar_catalogs();
        }
    }

    pub(crate) fn open_artifact(&mut self, handle: crate::ui::artifact_sidebar::ArtifactHandle) {
        let Some(mut artifact) = self.find_artifact(&handle) else {
            return;
        };
        if artifact.content.is_none() {
            if let Some(path) = artifact.path.as_ref() {
                artifact.content = std::fs::read_to_string(path).ok();
            }
        }
        self.ui.artifact_viewer =
            Some(crate::ui::modals::artifact_viewer::ArtifactViewerState::new(artifact));
    }

    pub(crate) fn edit_artifact(&mut self, handle: crate::ui::artifact_sidebar::ArtifactHandle) {
        let Some(artifact) = self.find_artifact(&handle) else {
            return;
        };
        let Some(path) = artifact.path.clone() else {
            self.ui
                .show_toast("Editor requires a saved file.".to_string());
            return;
        };
        match crate::ui::modals::editor_popup::EditorPopupState::new(&path) {
            Ok(state) => self.ui.editor_popup = Some(state),
            Err(error) => self.ui.show_toast(error),
        }
    }

    pub(crate) fn delete_artifact(&mut self, handle: crate::ui::artifact_sidebar::ArtifactHandle) {
        match &handle {
            crate::ui::artifact_sidebar::ArtifactHandle::Saved(path) => {
                let _ = std::fs::remove_file(path);
                self.refresh_saved_artifacts();
            }
            crate::ui::artifact_sidebar::ArtifactHandle::Memory(path) => {
                if let Some(artifact) = self.find_artifact(&handle) {
                    if let Some(physical_path) = artifact.path {
                        let _ = std::fs::remove_file(physical_path);
                    } else {
                        let _ = std::fs::remove_file(path);
                    }
                }
                self.refresh_memory_artifacts();
            }
            crate::ui::artifact_sidebar::ArtifactHandle::Vault(path) => {
                if let Some(vault) = &self.vault {
                    let full_path = vault.root.join(path);
                    let _ = std::fs::remove_file(full_path);
                    self.refresh_vault_artifacts();
                }
            }
            _ => {
                self.remove_temporary_artifact(&handle);
            }
        }
        if self
            .ui
            .artifact_viewer
            .as_ref()
            .is_some_and(|viewer| viewer.handle() == &handle)
        {
            self.ui.artifact_viewer = None;
        }
    }

    pub(crate) fn remove_temporary_artifact(
        &mut self,
        handle: &crate::ui::artifact_sidebar::ArtifactHandle,
    ) {
        if let Some(tab) = self.ui.tabs.get_mut(self.ui.active_tab) {
            tab.temporary_artifacts
                .retain(|artifact| &artifact.handle != handle);
        }
    }

    pub(crate) fn find_artifact(
        &self,
        handle: &crate::ui::artifact_sidebar::ArtifactHandle,
    ) -> Option<crate::ui::artifact_sidebar::ArtifactEntry> {
        self.ui
            .tabs
            .get(self.ui.active_tab)
            .and_then(|tab| {
                tab.temporary_artifacts
                    .iter()
                    .find(|artifact| &artifact.handle == handle)
                    .cloned()
            })
            .or_else(|| {
                self.ui
                    .saved_artifacts
                    .iter()
                    .find(|artifact| &artifact.handle == handle)
                    .cloned()
            })
            .or_else(|| {
                self.ui
                    .memory_artifacts
                    .iter()
                    .find(|artifact| &artifact.handle == handle)
                    .cloned()
            })
            .or_else(|| {
                self.ui
                    .vault_artifacts
                    .iter()
                    .find(|artifact| &artifact.handle == handle)
                    .cloned()
            })
    }
}

pub(crate) fn local_media_sources(content: &str) -> Vec<String> {
    let mut sources = Vec::new();
    let mut seen = HashSet::new();

    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("![") && trimmed.ends_with(')') {
            if let Some(start) = trimmed.find("](") {
                let source = trimmed[start + 2..trimmed.len().saturating_sub(1)]
                    .trim()
                    .trim_matches('<')
                    .trim_matches('>');
                if is_artifact_image(source) && seen.insert(source.to_string()) {
                    sources.push(source.to_string());
                }
            }
            continue;
        }

        if is_artifact_image(trimmed) && seen.insert(trimmed.to_string()) {
            sources.push(trimmed.to_string());
        }
    }

    sources
}

fn is_artifact_image(source: &str) -> bool {
    if !crate::ui::components::image_block::is_local_image_source(source) {
        return false;
    }
    let trimmed = source
        .trim()
        .trim_matches('<')
        .trim_matches('>')
        .strip_prefix("file://")
        .unwrap_or(source.trim());
    matches!(
        std::path::Path::new(trimmed)
            .extension()
            .and_then(|extension| extension.to_str())
            .map(str::to_ascii_lowercase)
            .as_deref(),
        Some("png" | "jpg" | "jpeg" | "gif" | "webp" | "bmp")
    )
}

fn strip_frontmatter(markdown: &str) -> &str {
    let trimmed = markdown.trim_start();
    let Some(after_dashes) = trimmed.strip_prefix("---") else {
        return markdown;
    };
    let Some(end) = after_dashes.find("\n---") else {
        return markdown;
    };
    after_dashes[end + 4..].trim_start_matches('\n').trim_end()
}
