use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GeneratedFile {
    pub id: u64,
    pub conversation_id: i64,
    pub name: String,
    pub content: String,
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
        })
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
    fn user_paths_expand_a_leading_tilde() {
        // Given
        let home = Path::new("/home/example");

        // When
        let expanded = super::expand_user_path(Path::new("~/artifacts/demo.md"), Some(home));

        // Then
        assert_eq!(expanded, home.join("artifacts/demo.md"));
    }
}
