use std::path::Path;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GeneratedFile {
    pub id: u64,
    pub conversation_id: i64,
    pub name: String,
    pub content: String,
}

impl GeneratedFile {
    pub fn maybe_from_response(
        id: u64,
        conversation_id: i64,
        prompt: &str,
        answer: &str,
    ) -> Option<Self> {
        if !requests_markdown_file(prompt) {
            return None;
        }

        let content = extract_markdown_body(answer).unwrap_or_else(|| answer.trim().to_string());
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
        let content = extract_markdown_body(answer).unwrap_or_else(|| answer.trim().to_string());
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

fn requests_markdown_file(prompt: &str) -> bool {
    let lower = prompt.to_ascii_lowercase();
    [
        ".md file",
        "markdown file",
        "markdown document",
        "write it to .md",
        "write it to a .md",
        "write this to .md",
        "save it as .md",
        "save this as .md",
    ]
    .iter()
    .any(|needle| lower.contains(needle))
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

fn extract_markdown_body(answer: &str) -> Option<String> {
    let markdown_start = answer
        .find("```markdown\n")
        .map(|idx| idx + "```markdown\n".len());
    let generic_start = answer.find("```\n").map(|idx| idx + "```\n".len());
    let start = markdown_start.or(generic_start)?;
    let rest = &answer[start..];
    let end = rest.find("\n```")?;
    let body = rest[..end].trim();
    if body.is_empty() {
        None
    } else {
        Some(body.to_string())
    }
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

    #[test]
    fn creates_markdown_file_from_prompt_and_response() {
        let file = GeneratedFile::maybe_from_response(
            1,
            42,
            "give me this specific information and write it to a report.md file",
            "```markdown\n# Report\n\nHello\n```",
        )
        .expect("expected markdown artifact");

        assert_eq!(file.name, "report.md");
        assert_eq!(file.content, "# Report\n\nHello");
    }

    #[test]
    fn ignores_regular_prompts() {
        let file = GeneratedFile::maybe_from_response(1, 42, "just answer normally", "# Hello");
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
}
