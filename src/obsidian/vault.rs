#![allow(dead_code)]
use crate::config::vault_profiles::VaultProfileId;
use color_eyre::Result;
use std::path::{Path, PathBuf};

use crate::vault::{
    VaultEntryKind, VaultFs, VaultProfileRegistration, VaultRelativePath, VaultSearchLimits,
    VaultWalkLimits, VaultWriteService,
};

pub struct Vault {
    pub root: PathBuf,
}

impl Vault {
    pub fn new(root: PathBuf) -> Self {
        Self { root }
    }

    pub fn exists(&self) -> bool {
        self.root.is_dir()
    }

    pub fn list_files(&self, path: Option<&Path>) -> Result<Vec<PathBuf>> {
        let start = path.map(VaultRelativePath::parse).transpose()?;
        let listing =
            VaultFs::open_profile(&self.root)?.list(start.as_ref(), VaultWalkLimits::default())?;
        if !listing.is_complete() {
            color_eyre::eyre::bail!("Vault listing exceeded its safety limits");
        }
        Ok(listing
            .entries
            .into_iter()
            .filter(|entry| entry.kind == VaultEntryKind::File)
            .map(|entry| self.root.join(entry.path))
            .collect())
    }

    pub fn read_file(&self, path: &Path) -> Result<String> {
        Ok(VaultFs::open_profile(&self.root)?.read_text(&VaultRelativePath::parse(path)?)?)
    }

    pub fn write_file(
        &self,
        profile_id: &VaultProfileId,
        path: &Path,
        content: &str,
    ) -> Result<()> {
        self.write_bytes(profile_id, path, content.as_bytes())
    }

    pub fn write_bytes(
        &self,
        profile_id: &VaultProfileId,
        path: &Path,
        content: &[u8],
    ) -> Result<()> {
        let relative = VaultRelativePath::parse(path)?;
        VaultFs::open_profile(&self.root)?.create_parent_directories(&relative)?;
        let service = VaultWriteService::new([VaultProfileRegistration::new(
            profile_id.clone(),
            self.root.clone(),
        )])?;
        service.write(profile_id, &relative, content)?;
        Ok(())
    }

    pub fn remove_file(&self, profile_id: &VaultProfileId, path: &Path) -> Result<()> {
        let relative = VaultRelativePath::parse(path)?;
        let service = VaultWriteService::new([VaultProfileRegistration::new(
            profile_id.clone(),
            self.root.clone(),
        )])?;
        service.remove(profile_id, &relative)?;
        Ok(())
    }

    pub fn search(&self, query: &str) -> Result<Vec<PathBuf>> {
        let result =
            VaultFs::open_profile(&self.root)?.search(query, VaultSearchLimits::default())?;
        if !result.is_complete() {
            color_eyre::eyre::bail!("Vault search exceeded its safety limits");
        }
        let mut paths: Vec<_> = result
            .matches
            .into_iter()
            .map(|entry| self.root.join(entry.path))
            .collect();
        paths.sort();
        paths.dedup();
        Ok(paths)
    }
}
