use std::collections::HashSet;
use std::path::Path;

use crate::config::vault_profiles::{
    VaultCatalog, VaultProfile, VaultProfileError, VaultProfileId,
};

use super::directory_picker::{canonical_directory, VaultDirectoryPickerState};
use super::manager::{
    ManagerOrigin, VaultProfileManagerState, VaultProfilePhase, VaultProfileUiCommand,
    VaultProfileUiError,
};

impl VaultProfileManagerState {
    pub(crate) fn begin_add(&mut self, start: &Path) -> Result<(), VaultProfileUiError> {
        self.picker = Some(VaultDirectoryPickerState::new(start)?);
        self.phase = VaultProfilePhase::Directory;
        self.pending_id = None;
        self.error = None;
        Ok(())
    }

    pub(crate) fn choose_directory(&mut self, path: &Path) -> Result<(), VaultProfileUiError> {
        let path = canonical_directory(path)?;
        self.input = path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("Vault")
            .to_owned();
        self.pending_path = Some(path);
        self.phase = VaultProfilePhase::Name;
        self.error = None;
        Ok(())
    }

    pub(crate) fn submit_name(
        &mut self,
        _bound: &HashSet<VaultProfileId>,
    ) -> VaultProfileUiCommand {
        let Some(path) = self.pending_path.as_ref() else {
            return VaultProfileUiCommand::None;
        };
        let profile = match VaultProfile::new(&self.input, path) {
            Ok(profile) => profile,
            Err(error) => return self.fail(error.into()),
        };
        let id = profile.id.clone();
        let mut catalog = self.catalog.clone();
        if let Err(error) = catalog.add(profile) {
            return self.fail(error.into());
        }
        let activate_profile_id = matches!(self.origin, ManagerOrigin::Launcher(_)).then_some(id);
        VaultProfileUiCommand::Persist {
            catalog,
            activate_profile_id,
        }
    }

    pub(crate) fn commit_catalog(&mut self, catalog: VaultCatalog) {
        self.catalog = catalog;
        self.selected = self
            .selected
            .min(self.catalog.profiles.len().saturating_sub(1));
        self.error = None;
        if self.origin == ManagerOrigin::Settings {
            self.phase = VaultProfilePhase::Profiles;
            self.pending_id = None;
            self.pending_path = None;
        }
    }

    pub(crate) fn set_error(&mut self, error: VaultProfileUiError) {
        self.error = Some(error);
    }

    pub(crate) fn rename(
        &mut self,
        id: &VaultProfileId,
        name: &str,
        _bound: &HashSet<VaultProfileId>,
    ) -> VaultProfileUiCommand {
        let mut catalog = self.catalog.clone();
        match catalog.rename(id, name) {
            Ok(()) => persist(catalog),
            Err(error) => self.fail(error.into()),
        }
    }

    pub(crate) fn set_default(&mut self, id: &VaultProfileId) -> VaultProfileUiCommand {
        let mut catalog = self.catalog.clone();
        match catalog.set_default(id) {
            Ok(()) => persist(catalog),
            Err(error) => self.fail(error.into()),
        }
    }

    pub(crate) fn remove(
        &mut self,
        id: &VaultProfileId,
        bound: &HashSet<VaultProfileId>,
    ) -> VaultProfileUiCommand {
        if bound.contains(id) {
            return self.fail(VaultProfileUiError::ProfileIsOpen { id: id.clone() });
        }
        let mut catalog = self.catalog.clone();
        match catalog.remove(id) {
            Ok(_) => persist(catalog),
            Err(error) => self.fail(error.into()),
        }
    }

    pub(crate) fn repoint(
        &mut self,
        id: &VaultProfileId,
        root: &Path,
        bound: &HashSet<VaultProfileId>,
    ) -> VaultProfileUiCommand {
        if bound.contains(id) {
            return self.fail(VaultProfileUiError::ProfileIsOpen { id: id.clone() });
        }
        let Some(current) = self.catalog.get(id) else {
            return self.fail(
                VaultProfileError::InvalidDefault {
                    id: Some(id.clone()),
                }
                .into(),
            );
        };
        if current.root == crate::protection::internal_vault_root() {
            return self.fail(VaultProfileError::BuiltinProfile.into());
        }
        let replacement =
            match VaultProfile::from_persisted(id.clone(), &current.display_name, root) {
                Ok(profile) if profile.available => profile,
                Ok(profile) => {
                    return self.fail(VaultProfileUiError::NotDirectory { path: profile.root });
                }
                Err(error) => return self.fail(error.into()),
            };
        if self
            .catalog
            .profiles
            .iter()
            .any(|(other_id, profile)| other_id != id && profile.root == replacement.root)
        {
            return self.fail(
                VaultProfileError::DuplicateRoot {
                    root: replacement.root,
                }
                .into(),
            );
        }
        let mut catalog = self.catalog.clone();
        catalog.profiles.insert(id.clone(), replacement);
        persist(catalog)
    }
}

fn persist(catalog: VaultCatalog) -> VaultProfileUiCommand {
    VaultProfileUiCommand::Persist {
        catalog,
        activate_profile_id: None,
    }
}
