use std::path::PathBuf;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum VaultLimitKind {
    Entries,
    Depth,
    Files,
    TotalBytes,
    FileBytes,
    Matches,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VaultLimitReached {
    pub kind: VaultLimitKind,
    pub path: Option<PathBuf>,
}

impl VaultLimitReached {
    pub(crate) fn new(kind: VaultLimitKind, path: Option<PathBuf>) -> Self {
        Self { kind, path }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VaultWalkLimits {
    pub max_entries: usize,
    pub max_depth: usize,
}

impl Default for VaultWalkLimits {
    fn default() -> Self {
        Self {
            max_entries: 50_000,
            max_depth: 64,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VaultSearchLimits {
    pub max_files: usize,
    pub max_total_bytes: u64,
    pub max_file_bytes: u64,
    pub max_matches: usize,
}

impl Default for VaultSearchLimits {
    fn default() -> Self {
        Self {
            max_files: 10_000,
            max_total_bytes: 256 * 1024 * 1024,
            max_file_bytes: 1024 * 1024,
            max_matches: 500,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{VaultSearchLimits, VaultWalkLimits};

    #[test]
    fn vault_confined_read_limits_have_required_defaults() {
        assert_eq!(
            VaultWalkLimits::default(),
            VaultWalkLimits {
                max_entries: 50_000,
                max_depth: 64,
            }
        );
        assert_eq!(
            VaultSearchLimits::default(),
            VaultSearchLimits {
                max_files: 10_000,
                max_total_bytes: 256 * 1024 * 1024,
                max_file_bytes: 1024 * 1024,
                max_matches: 500,
            }
        );
    }
}
