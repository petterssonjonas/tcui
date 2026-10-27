use std::ffi::OsString;
use std::path::{Path, PathBuf};

#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;

pub(super) struct TestEnvironment {
    pub(super) root: PathBuf,
    original_data_home: Option<OsString>,
    original_home: Option<OsString>,
    original_path: Option<OsString>,
}

impl TestEnvironment {
    pub(super) fn new(label: &str) -> std::io::Result<Self> {
        let root = std::env::temp_dir().join(format!(
            "tcui-{label}-{}-{}",
            std::process::id(),
            rand::random::<u64>()
        ));
        std::fs::create_dir_all(&root)?;
        let original_data_home = std::env::var_os("XDG_DATA_HOME");
        let original_home = std::env::var_os("HOME");
        let original_path = std::env::var_os("PATH");
        std::env::set_var("HOME", &root);
        std::env::set_var("XDG_DATA_HOME", root.join("data"));
        Ok(Self {
            root,
            original_data_home,
            original_home,
            original_path,
        })
    }

    pub(super) fn auth_path(&self) -> PathBuf {
        self.root.join(".codex").join("auth.json")
    }

    pub(super) fn write_external_auth(&self, contents: &str) -> std::io::Result<()> {
        let path = self.auth_path();
        let parent = path
            .parent()
            .ok_or_else(|| std::io::Error::other("auth path has no parent"))?;
        std::fs::create_dir_all(parent)?;
        std::fs::write(&path, contents)?;
        #[cfg(unix)]
        {
            let mut permissions = std::fs::metadata(&path)?.permissions();
            permissions.set_mode(0o600);
            std::fs::set_permissions(&path, permissions)?;
        }
        Ok(())
    }

    pub(super) fn prepend_path(&self, directory: &Path) {
        let previous = self.original_path.as_deref().unwrap_or_default();
        let mut paths = vec![directory.to_path_buf()];
        paths.extend(std::env::split_paths(previous));
        if let Ok(joined) = std::env::join_paths(paths) {
            std::env::set_var("PATH", joined);
        }
    }
}

impl Drop for TestEnvironment {
    fn drop(&mut self) {
        restore("XDG_DATA_HOME", self.original_data_home.take());
        restore("HOME", self.original_home.take());
        restore("PATH", self.original_path.take());
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

fn restore(name: &str, value: Option<OsString>) {
    match value {
        Some(value) => std::env::set_var(name, value),
        None => std::env::remove_var(name),
    }
}
