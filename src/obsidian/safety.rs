#![allow(dead_code)]
use chrono::Utc;
use color_eyre::Result;
use similar::TextDiff;
use std::path::Path;

pub struct Diff {
    pub path: String,
    pub old_content: String,
    pub new_content: String,
    pub status: DiffStatus,
}

pub enum DiffStatus {
    Pending,
    Accepted,
    Rejected,
}

pub struct SafetyLayer {
    pub backup_dir: std::path::PathBuf,
    pub retention_days: i64,
}

impl SafetyLayer {
    pub fn new(backup_dir: std::path::PathBuf) -> Self {
        Self {
            backup_dir,
            retention_days: 7,
        }
    }

    pub fn generate_diff(&self, old: &str, new: &str) -> String {
        let diff = TextDiff::from_lines(old, new);
        diff.unified_diff()
            .context_radius(3)
            .header("old", "new")
            .to_string()
    }

    pub fn create_backup(&self, path: &Path, content: &str) -> Result<std::path::PathBuf> {
        let timestamp = Utc::now().format("%Y%m%d_%H%M%S");
        let filename = format!(
            "{}.tcui.bak.{}",
            path.file_name().unwrap_or_default().to_string_lossy(),
            timestamp
        );
        let backup_path = self.backup_dir.join(&filename);

        std::fs::create_dir_all(&self.backup_dir)?;
        std::fs::write(&backup_path, content)?;

        Ok(backup_path)
    }

    pub fn parse_diff_for_review(&self, diff: &str) -> DiffReview {
        let lines: Vec<DiffLine> = diff
            .lines()
            .map(|line| {
                let (sign, text) = if let Some(rest) = line.strip_prefix('+') {
                    ("+".to_string(), rest.to_string())
                } else if let Some(rest) = line.strip_prefix('-') {
                    ("-".to_string(), rest.to_string())
                } else {
                    (" ".to_string(), line.to_string())
                };
                DiffLine { sign, text }
            })
            .collect();

        DiffReview { lines }
    }
}

pub struct DiffReview {
    pub lines: Vec<DiffLine>,
}

pub struct DiffLine {
    pub sign: String,
    pub text: String,
}

#[cfg(test)]
mod tests {
    use super::SafetyLayer;
    use std::path::Path;

    #[test]
    fn backup_preserves_original_content_and_diff_captures_changes() {
        let backup_dir =
            std::env::temp_dir().join(format!("tcui-safety-{}", rand::random::<u64>()));
        let safety = SafetyLayer::new(backup_dir.clone());
        let original = "line one\nline two\n";
        let updated = "line one\nline two changed\nline three\n";

        let backup_path = safety
            .create_backup(Path::new("note.md"), original)
            .expect("backup created");

        assert!(backup_path.starts_with(&backup_dir));
        assert_eq!(
            std::fs::read_to_string(&backup_path).expect("read backup"),
            original
        );

        let diff = safety.generate_diff(original, updated);
        assert!(diff.contains("-line two"));
        assert!(diff.contains("+line two changed"));

        std::fs::remove_dir_all(&backup_dir).expect("cleanup");
    }
}
