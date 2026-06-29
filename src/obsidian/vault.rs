#![allow(dead_code)]
use color_eyre::Result;
use std::path::{Path, PathBuf};
use walkdir::WalkDir;

pub struct Vault {
    pub root: PathBuf,
}

impl Vault {
    pub fn new(root: PathBuf) -> Self {
        Self { root }
    }

    pub fn exists(&self) -> bool {
        self.root.exists() && self.root.is_dir()
    }

    pub fn list_files(&self, path: Option<&Path>) -> Result<Vec<PathBuf>> {
        let root = match path {
            Some(p) => self.root.join(p),
            None => self.root.clone(),
        };

        let mut files = Vec::new();
        for entry in WalkDir::new(&root).into_iter().filter_map(|e| e.ok()) {
            if entry.file_type().is_file() {
                files.push(entry.path().to_path_buf());
            }
        }
        Ok(files)
    }

    pub fn read_file(&self, path: &Path) -> Result<String> {
        let full_path = self.root.join(path);
        Ok(std::fs::read_to_string(full_path)?)
    }

    pub fn write_file(&self, path: &Path, content: &str) -> Result<()> {
        let full_path = self.root.join(path);
        if let Some(parent) = full_path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        Ok(std::fs::write(full_path, content)?)
    }

    pub fn search(&self, query: &str) -> Result<Vec<PathBuf>> {
        let query_lower = query.to_lowercase();
        let mut results = Vec::new();

        for entry in WalkDir::new(&self.root).into_iter().filter_map(|e| e.ok()) {
            if entry.file_type().is_file() {
                if let Ok(content) = std::fs::read_to_string(entry.path()) {
                    if content.to_lowercase().contains(&query_lower) {
                        results.push(entry.path().to_path_buf());
                    }
                }
            }
        }
        Ok(results)
    }
}
