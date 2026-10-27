use std::ops::Range;

use pulldown_cmark::{CodeBlockKind, Event, Parser, Tag};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandBlock {
    pub language: String,
    pub code: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandFence {
    pub source_range: Range<usize>,
    pub language: &'static str,
}

/// Find explicitly runnable, top-level fences without copying their payloads.
pub fn command_fences(source: &str) -> Vec<CommandFence> {
    command_ranges(source).map(|(fence, _)| fence).collect()
}

/// Copy only the selected runnable block, using the same order as `command_fences`.
pub fn command_block(source: &str, index: usize) -> Option<CommandBlock> {
    let (fence, payload) = command_ranges(source).nth(index)?;
    Some(CommandBlock {
        language: fence.language.to_owned(),
        code: source[payload].to_owned(),
    })
}

fn command_ranges(source: &str) -> impl Iterator<Item = (CommandFence, Range<usize>)> + '_ {
    let mut depth = 0;
    Parser::new(source)
        .into_offset_iter()
        .filter_map(move |(event, range)| match event {
            Event::Start(tag) => {
                let top_level = depth == 0;
                depth += 1;
                if top_level && matches!(tag, Tag::CodeBlock(CodeBlockKind::Fenced(_))) {
                    validate_fence(source, range)
                } else {
                    None
                }
            }
            Event::End(_) => {
                depth -= 1;
                None
            }
            _ => None,
        })
}

fn validate_fence(
    source: &str,
    source_range: Range<usize>,
) -> Option<(CommandFence, Range<usize>)> {
    // Markdown permits indentation and nested containers; runnable fences do not.
    if source_range.start != 0 && source.as_bytes()[source_range.start - 1] != b'\n' {
        return None;
    }
    let raw = &source[source_range.clone()];
    let (opening, rest) = raw.split_once('\n')?;
    let opening = opening.strip_suffix('\r').unwrap_or(opening);
    let marker = *opening.as_bytes().first()?;
    if !matches!(marker, b'`' | b'~') {
        return None;
    }
    let marker_count = opening.bytes().take_while(|&byte| byte == marker).count();
    if marker_count < 3 {
        return None;
    }
    let info = &opening[marker_count..];
    if info.chars().any(|ch| ch.is_control() && ch != '\t') {
        return None;
    }
    let mut tokens = info.split_ascii_whitespace();
    let name = tokens.next()?;
    let language = ["sh", "bash", "shell", "zsh", "fish"]
        .into_iter()
        .find(|known| name.eq_ignore_ascii_case(known))?;
    if tokens.next() != Some("tcui-run") || tokens.next().is_some() {
        return None;
    }

    // Offset ranges exclude the closing line ending. An unclosed Markdown fence
    // still emits a code block, so require the actual matching delimiter here.
    let closing_start = rest.rfind('\n').map_or(0, |index| index + 1);
    let closing = &rest[closing_start..];
    let closing_count = closing.bytes().take_while(|&byte| byte == marker).count();
    if closing_count < marker_count || !closing[closing_count..].bytes().all(|byte| byte == b' ') {
        return None;
    }
    let payload_start = source_range.start + raw.len() - rest.len();
    let payload = payload_start..payload_start + closing_start;
    let code = &source[payload.clone()];
    if code.trim().is_empty()
        || code.char_indices().any(|(index, ch)| {
            ch.is_control()
                && !matches!(ch, '\n' | '\t')
                && !(ch == '\r' && code.as_bytes().get(index + 1) == Some(&b'\n'))
        })
    {
        return None;
    }
    Some((
        CommandFence {
            source_range,
            language,
        },
        payload,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn assert_rejected(source: &str) {
        assert!(command_fences(source).is_empty(), "{source:?}");
        assert!(command_block(source, 0).is_none(), "{source:?}");
    }

    #[test]
    fn requires_exact_explicit_intent_and_supported_shell() {
        for info in [
            "sh",
            "bash",
            "shell",
            "zsh",
            "fish",
            "",
            "text",
            "output",
            "python tcui-run",
            "bash tcui-run extra",
            "tcui-run bash",
            "bash TCUI-RUN",
            "bash tcui-runner",
            "bash tcui\\-run",
            "bash\u{a0}tcui-run",
        ] {
            assert_rejected(&format!("```{info}\npwd\n```"));
        }
        for language in ["sh", "bash", "shell", "zsh", "fish", "BASH"] {
            let source = format!("```{language} tcui-run\npwd\n```");
            assert_eq!(
                command_fences(&source)[0].language,
                language.to_ascii_lowercase()
            );
            assert_eq!(command_block(&source, 0).unwrap().code, "pwd\n");
        }
        assert_eq!(
            command_block("``` bash\t tcui-run  \npwd\n```", 0)
                .unwrap()
                .language,
            "bash"
        );
    }

    #[test]
    fn rejects_unclosed_empty_and_control_bearing_fences() {
        for source in [
            "```sh tcui-run\npwd\n",
            "```sh tcui-run\npwd",
            "```sh tcui-run\n> ```",
            "```sh tcui-run\n```",
            "```sh tcui-run\n \t\n```",
            "````sh tcui-run\npwd\n```",
            "~~~sh tcui-run\npwd\n```",
            "```sh tcui-run\npwd\n``` trailing",
            "```sh tcui-run\nprintf '\u{1b}]52;c;hidden'\n```",
            "```sh tcui-run\nprintf '\0'\n```",
            "```sh tcui-run\nprintf '\u{7f}'\n```",
            "```sh tcui-run\nprintf '\u{85}'\n```",
            "```sh tcui-run\nprintf 'hidden\rvisible'\n```",
            "```sh\u{b}tcui-run\npwd\n```",
        ] {
            assert_rejected(source);
        }
    }

    #[test]
    fn excludes_quotes_lists_indentation_and_nested_example_fences() {
        for source in [
            "> ```bash tcui-run\n> pwd\n> ```",
            "1. Step\n\n   ```bash tcui-run\n   pwd\n   ```",
            "- ```bash tcui-run\n  pwd\n  ```",
            "  ```bash tcui-run\npwd\n```",
            "```bash tcui-run\npwd\n  ```",
            "    ```bash tcui-run\n    pwd\n    ```",
            "````text\n```bash tcui-run\npwd\n```\n````",
            "````bash\n```bash tcui-run\npwd\n```\n````",
            "The syntax is `bash tcui-run`; the command is `pwd`.",
        ] {
            assert_rejected(source);
        }
    }

    #[test]
    fn preserves_crlf_unicode_tabs_blank_lines_and_heredoc_payload_exactly() {
        let code = concat!(
            "(\r\n",
            "\tprintf '%s\\n' 'héllo 世界'\r\n",
            "\r\n",
            "\tcat <<'END'\r\n",
            "$HOME stays literal; `not expanded`\r\n",
            "END\r\n",
            ")\r\n",
        );
        let source = format!("Préface.\r\n```bash tcui-run\r\n{code}```\r\nAfter.");
        assert_eq!(
            command_block(&source, 0),
            Some(CommandBlock {
                language: "bash".into(),
                code: code.into(),
            })
        );
        let fence = &command_fences(&source)[0];
        assert_eq!(
            &source[fence.source_range.clone()],
            format!("```bash tcui-run\r\n{code}```")
        );
    }

    #[test]
    fn metadata_ranges_and_selected_payloads_have_matching_ordinals() {
        let first = "~~~sh tcui-run\npwd\n~~~~  ";
        let second = "````fish tcui-run\nprintf '%s\\n' 'λ'\n````";
        let source = format!(
            "文.\n```bash\nnot recommended\n```\n{first}\nBetween.\n\
             ```text\nsample output\n```\n{second}\n```zsh tcui-run\nunfinished"
        );
        let fences = command_fences(&source);
        let first_start = source.find(first).unwrap();
        let second_start = source.find(second).unwrap();
        assert_eq!(
            fences,
            vec![
                CommandFence {
                    source_range: first_start..first_start + first.len(),
                    language: "sh",
                },
                CommandFence {
                    source_range: second_start..second_start + second.len(),
                    language: "fish",
                },
            ]
        );
        for (index, expected) in ["pwd\n", "printf '%s\\n' 'λ'\n"].into_iter().enumerate() {
            let block = command_block(&source, index).unwrap();
            assert_eq!(block.language, fences[index].language);
            assert_eq!(block.code, expected);
            assert_eq!(
                command_block(&source[fences[index].source_range.clone()], 0),
                Some(block)
            );
        }
        assert!(command_block(&source, fences.len()).is_none());
    }

    #[test]
    fn explicit_marker_not_surrounding_prose_decides_intent() {
        let source = "Discussion only.\n```bash tcui-run\npwd\n```";
        assert_eq!(command_fences(source).len(), 1);
        assert_eq!(command_block(source, 0).unwrap().code, "pwd\n");
        assert_rejected("Run this recommended command:\n```bash\npwd\n```");
    }
}
