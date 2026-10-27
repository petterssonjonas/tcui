use std::ffi::OsStr;
use std::fmt;
use std::path::{Component, Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum VaultPathError {
    #[error("vault-relative path cannot be empty")]
    Empty,
    #[error("vault-relative path cannot be absolute")]
    Absolute,
    #[error("vault-relative path cannot contain a parent component")]
    ParentTraversal,
    #[error("vault-relative path cannot contain a platform path prefix")]
    PlatformPrefix,
    #[error("vault-relative path contains reserved device name: {name}")]
    DeviceName { name: String },
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct VaultRelativePath(PathBuf);

impl VaultRelativePath {
    pub fn parse(path: &Path) -> Result<Self, VaultPathError> {
        reject_portable_prefix(path)?;
        let mut relative = PathBuf::new();

        for component in path.components() {
            match component {
                Component::Prefix(_) => return Err(VaultPathError::PlatformPrefix),
                Component::RootDir => return Err(VaultPathError::Absolute),
                Component::ParentDir => return Err(VaultPathError::ParentTraversal),
                Component::CurDir => {}
                Component::Normal(name) => {
                    reject_device_name(name)?;
                    relative.push(name);
                }
            }
        }

        if relative.as_os_str().is_empty() {
            return Err(VaultPathError::Empty);
        }
        Ok(Self(relative))
    }

    pub fn as_path(&self) -> &Path {
        &self.0
    }

    pub fn into_path_buf(self) -> PathBuf {
        self.0
    }

    pub fn display(&self) -> impl fmt::Display + '_ {
        self.0.display()
    }
}

impl AsRef<Path> for VaultRelativePath {
    fn as_ref(&self) -> &Path {
        self.as_path()
    }
}

fn reject_portable_prefix(path: &Path) -> Result<(), VaultPathError> {
    let Some(text) = path.as_os_str().to_str() else {
        return Ok(());
    };
    let bytes = text.as_bytes();
    let drive_relative = bytes.len() >= 2 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':';
    if drive_relative || text.starts_with("\\\\") {
        return Err(VaultPathError::PlatformPrefix);
    }
    Ok(())
}

fn reject_device_name(name: &OsStr) -> Result<(), VaultPathError> {
    let Some(text) = name.to_str() else {
        return Ok(());
    };
    let stem = text
        .trim_end_matches([' ', '.'])
        .split('.')
        .next()
        .unwrap_or_default()
        .trim_end_matches(':');
    let upper = stem.to_ascii_uppercase();
    let reserved = matches!(upper.as_str(), "CON" | "PRN" | "AUX" | "NUL")
        || is_numbered_device(&upper, "COM")
        || is_numbered_device(&upper, "LPT");
    if reserved {
        return Err(VaultPathError::DeviceName {
            name: text.to_owned(),
        });
    }
    Ok(())
}

fn is_numbered_device(name: &str, prefix: &str) -> bool {
    name.strip_prefix(prefix)
        .is_some_and(|suffix| matches!(suffix.as_bytes(), [b'1'..=b'9']))
}

#[cfg(test)]
mod tests {
    use std::ffi::OsString;
    use std::path::{Path, PathBuf};

    use super::{VaultPathError, VaultRelativePath};

    #[test]
    fn vault_confined_read_path_parser_rejects_empty_and_dot_paths() {
        for path in [Path::new(""), Path::new(".")] {
            assert!(matches!(
                VaultRelativePath::parse(path),
                Err(VaultPathError::Empty)
            ));
        }
    }

    #[test]
    fn vault_confined_read_path_parser_rejects_absolute_and_parent_paths() {
        assert!(matches!(
            VaultRelativePath::parse(Path::new("/outside.md")),
            Err(VaultPathError::Absolute)
        ));
        for path in [Path::new(".."), Path::new("notes/../outside.md")] {
            assert!(matches!(
                VaultRelativePath::parse(path),
                Err(VaultPathError::ParentTraversal)
            ));
        }
    }

    #[test]
    fn vault_confined_read_path_parser_rejects_platform_prefixes_and_device_names() {
        for path in [
            Path::new(r"C:\outside.md"),
            Path::new(r"\\server\share\outside.md"),
            Path::new(r"\\?\C:\outside.md"),
            Path::new(r"\\.\NUL"),
        ] {
            assert!(matches!(
                VaultRelativePath::parse(path),
                Err(VaultPathError::PlatformPrefix)
            ));
        }

        for path in [
            Path::new("CON"),
            Path::new("nul.txt"),
            Path::new("notes/COM1.md"),
            Path::new("LPT9"),
        ] {
            assert!(matches!(
                VaultRelativePath::parse(path),
                Err(VaultPathError::DeviceName { .. })
            ));
        }
    }

    #[test]
    fn vault_confined_read_path_parser_removes_dot_components() {
        let parsed = VaultRelativePath::parse(Path::new("notes/./today.md"))
            .expect("dot components should normalize");

        assert_eq!(parsed.as_path(), Path::new("notes/today.md"));
    }

    #[cfg(unix)]
    #[test]
    fn vault_confined_read_path_parser_preserves_non_utf8_components() {
        use std::os::unix::ffi::OsStringExt;

        let original = PathBuf::from(OsString::from_vec(vec![b'n', b'o', 0x80, b'e']));
        let parsed = VaultRelativePath::parse(&original).expect("native path should remain valid");

        assert_eq!(parsed.as_path(), original);
    }

    #[test]
    fn vault_confined_read_path_display_is_explicitly_lossy() {
        let parsed = VaultRelativePath::parse(Path::new("notes/today.md"))
            .expect("ordinary relative path should parse");

        assert_eq!(parsed.display().to_string(), "notes/today.md");
    }

    #[test]
    fn vault_confined_read_path_accepts_native_os_strings_without_string_round_trip() {
        let original = PathBuf::from(OsString::from("folder")).join(OsString::from("note.md"));
        let parsed = VaultRelativePath::parse(&original).expect("native components should parse");

        assert_eq!(parsed.into_path_buf(), original);
    }
}
