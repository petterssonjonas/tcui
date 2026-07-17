mod cleanup;
mod marker;
mod path_safety;

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use crate::config::AppConfig;
use crate::storage::cache_paths::TcuiCachePaths;
use crate::storage::paths::{ensure_directory, TcuiDataPaths};

use marker::{MarkerWriter, MigrationLock};
use path_safety::{normalize_absolute, roots_overlap};

#[derive(Debug, Clone)]
pub(crate) struct LegacyResetEnvironment {
    pub(crate) data_paths: TcuiDataPaths,
    pub(crate) cache_paths: TcuiCachePaths,
    pub(crate) home_dir: PathBuf,
}

#[derive(Debug, Clone)]
pub(crate) struct ChatSavedLegacyReset {
    environment: LegacyResetEnvironment,
    saved_root: PathBuf,
    protected_roots: BTreeSet<PathBuf>,
    marker_writer: MarkerWriter,
}

pub(crate) struct LegacyResetStartup {
    pub(crate) saved_root: PathBuf,
    pub(crate) data_paths: TcuiDataPaths,
    pub(crate) warning: Option<LegacyResetError>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum LegacyResetOutcome {
    Completed,
    AlreadyCompleted,
}

#[derive(Debug, thiserror::Error)]
pub(crate) enum LegacyResetError {
    #[error("application configuration is unavailable")]
    ConfigurationUnavailable,
    #[error("TCUI data directory is unavailable")]
    DataDirectoryUnavailable,
    #[error("TCUI cache directory is unavailable")]
    CacheDirectoryUnavailable,
    #[error("home directory is unavailable")]
    HomeDirectoryUnavailable,
    #[error("Saved files root must be an absolute path: {path}")]
    SavedRootNotAbsolute { path: PathBuf },
    #[error("protected Vault root must be an absolute path: {path}")]
    ProtectedRootNotAbsolute { path: PathBuf },
    #[error("refusing unsafe Saved files root: {path}")]
    UnsafeSavedRoot { path: PathBuf },
    #[error("legacy reset root {destructive_root} overlaps protected Vault root {protected_root}")]
    ProtectedRootOverlap {
        destructive_root: PathBuf,
        protected_root: PathBuf,
    },
    #[error("legacy Chat/Saved reset is busy")]
    MigrationBusy,
    #[error("legacy reset marker is not a regular file: {path}")]
    InvalidMarker { path: PathBuf },
    #[cfg(test)]
    #[error("injected legacy reset marker write failure")]
    InjectedMarkerWriteFailure,
    #[error(transparent)]
    Io(#[from] std::io::Error),
}

impl ChatSavedLegacyReset {
    pub(crate) fn startup(
        config: Option<&AppConfig>,
        active_vault_root: Option<&Path>,
    ) -> LegacyResetStartup {
        let data_paths = TcuiDataPaths::discover();
        let saved_root = config
            .and_then(|config| config.saved_files_dir.as_deref())
            .map(str::trim)
            .filter(|path| !path.is_empty())
            .map(Path::new)
            .map(|path| expand_home(path, dirs::home_dir().as_deref().unwrap_or(Path::new("~"))))
            .unwrap_or_else(|| data_paths.root.join("artifacts"));

        #[cfg(test)]
        if std::env::var_os("TCUI_TEST_CHAT_SAVED_LEGACY_RESET").is_none() {
            return LegacyResetStartup {
                saved_root,
                data_paths,
                warning: None,
            };
        }

        let Some(config) = config else {
            return LegacyResetStartup {
                saved_root,
                data_paths,
                warning: Some(LegacyResetError::ConfigurationUnavailable),
            };
        };
        match Self::discover(config, active_vault_root) {
            Ok(reset) => LegacyResetStartup {
                saved_root: reset.saved_root().to_path_buf(),
                data_paths: reset.data_paths().clone(),
                warning: reset.run().err(),
            },
            Err(error) => LegacyResetStartup {
                saved_root,
                data_paths,
                warning: Some(error),
            },
        }
    }

    pub(crate) fn discover(
        config: &AppConfig,
        active_vault_root: Option<&Path>,
    ) -> Result<Self, LegacyResetError> {
        let data_home = dirs::data_dir().ok_or(LegacyResetError::DataDirectoryUnavailable)?;
        let cache_home = dirs::cache_dir().ok_or(LegacyResetError::CacheDirectoryUnavailable)?;
        let home_dir = dirs::home_dir().ok_or(LegacyResetError::HomeDirectoryUnavailable)?;
        Self::prepare(
            config,
            active_vault_root,
            LegacyResetEnvironment {
                data_paths: TcuiDataPaths::from_root(data_home.join("tcui")),
                cache_paths: TcuiCachePaths::from_root(cache_home.join("tcui")),
                home_dir,
            },
        )
    }

    pub(crate) fn prepare(
        config: &AppConfig,
        active_vault_root: Option<&Path>,
        environment: LegacyResetEnvironment,
    ) -> Result<Self, LegacyResetError> {
        let saved_root = match config.saved_files_dir.as_deref().map(str::trim) {
            Some(path) if !path.is_empty() => expand_home(Path::new(path), &environment.home_dir),
            Some(_) | None => environment.data_paths.root.join("artifacts"),
        };
        if !saved_root.is_absolute() {
            return Err(LegacyResetError::SavedRootNotAbsolute { path: saved_root });
        }
        let saved_root = normalize_absolute(&saved_root)?;
        let protected_roots = protected_roots(config, active_vault_root, &environment)?;

        Ok(Self {
            environment,
            saved_root,
            protected_roots,
            marker_writer: MarkerWriter::default(),
        })
    }

    pub(crate) fn saved_root(&self) -> &Path {
        &self.saved_root
    }

    pub(crate) fn data_paths(&self) -> &TcuiDataPaths {
        &self.environment.data_paths
    }

    pub(crate) fn run(&self) -> Result<LegacyResetOutcome, LegacyResetError> {
        let _lock = MigrationLock::acquire(&self.environment.data_paths)?;
        if marker::is_complete(&self.environment.data_paths)? {
            return Ok(LegacyResetOutcome::AlreadyCompleted);
        }
        self.validate_roots()?;

        cleanup::remove_legacy_cache(&self.environment.cache_paths.chat_files_dir)?;
        cleanup::remove_saved_root_files(&self.saved_root)?;
        ensure_directory(&self.saved_root)?;
        ensure_directory(&self.environment.data_paths.chat_file_links_dir)?;
        self.marker_writer.write(&self.environment.data_paths)?;
        Ok(LegacyResetOutcome::Completed)
    }

    fn validate_roots(&self) -> Result<(), LegacyResetError> {
        let home = normalize_absolute(&self.environment.home_dir)?;
        if path_safety::is_filesystem_root(&self.saved_root) || self.saved_root == home {
            return Err(LegacyResetError::UnsafeSavedRoot {
                path: self.saved_root.clone(),
            });
        }
        let cache_root = normalize_absolute(&self.environment.cache_paths.chat_files_dir)?;
        for destructive_root in [&self.saved_root, &cache_root] {
            if let Some(protected_root) = self
                .protected_roots
                .iter()
                .find(|protected_root| roots_overlap(destructive_root, protected_root))
            {
                return Err(LegacyResetError::ProtectedRootOverlap {
                    destructive_root: destructive_root.clone(),
                    protected_root: protected_root.clone(),
                });
            }
        }
        Ok(())
    }

    #[cfg(test)]
    fn fail_next_marker_write(&self) {
        self.marker_writer.fail_next();
    }
}

fn protected_roots(
    config: &AppConfig,
    active_vault_root: Option<&Path>,
    environment: &LegacyResetEnvironment,
) -> Result<BTreeSet<PathBuf>, LegacyResetError> {
    let mut roots = Vec::with_capacity(config.vaults.len() + 3);
    roots.extend(config.vaults.values().map(|profile| profile.root.clone()));
    if let Some(path) = config.vault_path.as_deref().map(str::trim) {
        if !path.is_empty() {
            roots.push(expand_home(Path::new(path), &environment.home_dir));
        }
    }
    if let Some(path) = active_vault_root {
        roots.push(path.to_path_buf());
    }
    roots.push(environment.data_paths.root.join("vault"));

    roots
        .into_iter()
        .map(|path| {
            if !path.is_absolute() {
                return Err(LegacyResetError::ProtectedRootNotAbsolute { path });
            }
            normalize_absolute(&path).map_err(LegacyResetError::from)
        })
        .collect()
}

fn expand_home(path: &Path, home: &Path) -> PathBuf {
    if path == Path::new("~") {
        return home.to_path_buf();
    }
    path.strip_prefix("~/")
        .map(|relative| home.join(relative))
        .unwrap_or_else(|_| path.to_path_buf())
}

#[cfg(test)]
mod tests;
