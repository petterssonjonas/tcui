use std::{
    fs::OpenOptions,
    io::Write,
    path::{Path, PathBuf},
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GeneratedFile {
    pub id: u64,
    pub conversation_id: i64,
    pub name: String,
    pub content: String,
    pub saved_path: Option<PathBuf>,
}

impl GeneratedFile {
    pub fn from_skill_response(
        id: u64,
        conversation_id: i64,
        prompt: &str,
        answer: &str,
    ) -> Option<Self> {
        if !crate::skills::mentions(prompt)
            .iter()
            .any(|mention| mention == "save")
        {
            return None;
        }
        let content = complete_markdown_body(answer);
        if content.is_empty() {
            return None;
        }
        let name = requested_filename(prompt)
            .or_else(|| heading_filename(&content))
            .unwrap_or_else(|| fallback_filename(prompt));
        Some(Self {
            id,
            conversation_id,
            name,
            content,
            saved_path: None,
        })
    }

    pub fn save_to(mut self, base_dir: &Path) -> std::io::Result<Self> {
        std::fs::create_dir_all(base_dir)?;
        let path = available_path(base_dir, &self.name)?;
        let mut output = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)?;
        output.write_all(self.content.as_bytes())?;
        output.sync_all()?;
        self.saved_path = Some(path);
        Ok(self)
    }
}

fn requested_filename(prompt: &str) -> Option<String> {
    prompt.split_whitespace().find_map(|token| {
        let cleaned = token.trim_matches(|c: char| {
            matches!(
                c,
                '"' | '\'' | '`' | ',' | '.' | ';' | ':' | '(' | ')' | '[' | ']'
            )
        });
        let lower = cleaned.to_ascii_lowercase();
        if !lower.ends_with(".md") {
            return None;
        }

        Path::new(cleaned)
            .file_name()
            .and_then(|name| name.to_str())
            .map(|name| {
                let stem = name.strip_suffix(".md").unwrap_or(name);
                let slug = sanitize_slug(stem);
                if slug.is_empty() {
                    String::new()
                } else {
                    format!("{slug}.md")
                }
            })
            .filter(|name| !name.is_empty())
    })
}

fn complete_markdown_body(answer: &str) -> String {
    let trimmed = answer.trim();
    for opening in ["```markdown\n", "```md\n", "```\n"] {
        if let Some(body) = trimmed
            .strip_prefix(opening)
            .and_then(|body| body.strip_suffix("\n```"))
        {
            return body.trim().to_string();
        }
    }
    trimmed.to_string()
}

fn available_path(base_dir: &Path, name: &str) -> std::io::Result<PathBuf> {
    let requested = base_dir.join(name);
    if !requested.exists() {
        return Ok(requested);
    }
    let path = Path::new(name);
    let stem = path
        .file_stem()
        .and_then(|value| value.to_str())
        .unwrap_or("artifact");
    let extension = path.extension().and_then(|value| value.to_str());
    for suffix in 2..=u32::MAX {
        let candidate = match extension {
            Some(extension) => base_dir.join(format!("{stem}-{suffix}.{extension}")),
            None => base_dir.join(format!("{stem}-{suffix}")),
        };
        if !candidate.exists() {
            return Ok(candidate);
        }
    }
    Err(std::io::Error::new(
        std::io::ErrorKind::AlreadyExists,
        "artifact directory has no available filename",
    ))
}

pub(crate) fn expand_user_path(path: &Path, home: Option<&Path>) -> PathBuf {
    if path == Path::new("~") {
        return home
            .map(Path::to_path_buf)
            .unwrap_or_else(|| path.to_path_buf());
    }
    path.strip_prefix("~/")
        .ok()
        .and_then(|relative| home.map(|home| home.join(relative)))
        .unwrap_or_else(|| path.to_path_buf())
}

fn heading_filename(content: &str) -> Option<String> {
    content.lines().find_map(|line| {
        let trimmed = line.trim();
        trimmed
            .strip_prefix("# ")
            .map(sanitize_filename)
            .filter(|name| !name.is_empty())
    })
}

fn fallback_filename(prompt: &str) -> String {
    let stem = prompt
        .split_whitespace()
        .take(5)
        .collect::<Vec<_>>()
        .join(" ")
        .to_ascii_lowercase();
    let stem = sanitize_filename(&stem);
    if stem.is_empty() {
        "generated-file.md".to_string()
    } else {
        stem
    }
}

fn sanitize_filename(input: &str) -> String {
    let stem = sanitize_slug(input);
    if stem.is_empty() {
        String::new()
    } else {
        format!("{stem}.md")
    }
}

fn sanitize_slug(input: &str) -> String {
    let mut out = String::new();
    let mut last_dash = false;

    for ch in input.chars() {
        if ch.is_ascii_alphanumeric() {
            out.push(ch.to_ascii_lowercase());
            last_dash = false;
        } else if matches!(ch, ' ' | '-' | '_' | '/' | '\\') && !last_dash {
            out.push('-');
            last_dash = true;
        }
    }

    let stem = out.trim_matches('-');
    stem.to_string()
}

#[cfg(test)]
mod tests {
    use super::GeneratedFile;
    use std::path::Path;

    #[test]
    fn regular_markdown_responses_are_not_artifacts() {
        // Given / When
        let file =
            GeneratedFile::from_skill_response(1, 42, "Show me a Markdown demo", "# Demo\n\nText");

        // Then
        assert!(file.is_none());
    }

    #[test]
    fn creates_sidebar_artifact_for_save_skill() {
        // Given
        let prompt = "@save summarize the release";
        let answer = "# Release summary\n\nLocal inference is available.";

        // When
        let file = GeneratedFile::from_skill_response(2, 42, prompt, answer)
            .expect("expected @save artifact");

        // Then
        assert_eq!(file.name, "release-summary.md");
        assert_eq!(file.content, answer);
    }

    #[test]
    fn creates_sidebar_artifact_when_save_mention_has_punctuation() {
        // Given
        let prompt = "Please use (@save), then summarize the release.";

        // When
        let file = GeneratedFile::from_skill_response(3, 42, prompt, "# Release")
            .expect("expected punctuation-delimited @save artifact");

        // Then
        assert_eq!(file.name, "release.md");
    }

    #[test]
    fn save_skill_preserves_the_complete_markdown_response() {
        // Given
        let answer = "# Headers\n\nText\n\n```\ncode\n```\n\n# Lists\n\n- one\n- two";

        // When
        let file = GeneratedFile::from_skill_response(4, 42, "@save this demo", answer)
            .expect("expected @save artifact");

        // Then
        assert_eq!(file.content, answer);
    }

    #[test]
    fn save_skill_writes_the_artifact_to_the_configured_directory() {
        // Given
        let root =
            std::env::temp_dir().join(format!("tcui-save-artifact-{}", rand::random::<u64>()));
        let file =
            GeneratedFile::from_skill_response(5, 42, "@save report.md", "# Report\n\nComplete.")
                .expect("expected @save artifact");

        // When
        let saved = file.save_to(&root).expect("save artifact");

        // Then
        let path = saved.saved_path.expect("saved path");
        assert_eq!(
            std::fs::read_to_string(&path).expect("read saved artifact"),
            "# Report\n\nComplete."
        );
        std::fs::remove_dir_all(root).expect("cleanup artifact directory");
    }

    #[test]
    fn user_paths_expand_a_leading_tilde() {
        // Given
        let home = Path::new("/home/example");

        // When
        let expanded = super::expand_user_path(Path::new("~/artifacts/demo.md"), Some(home));

        // Then
        assert_eq!(expanded, home.join("artifacts/demo.md"));
    }
}
