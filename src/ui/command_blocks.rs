use pulldown_cmark::{CodeBlockKind, Event, Parser, Tag, TagEnd};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandBlock {
    pub language: String,
    pub code: String,
}

pub fn shell_blocks(source: &str) -> Vec<CommandBlock> {
    scan(source, true).1
}

pub fn has_shell_blocks(source: &str) -> bool {
    scan(source, false).0
}

// Presence checks avoid copying code on every transcript redraw. Extraction uses
// the same parser and closure checks as the review path.
fn scan(source: &str, collect: bool) -> (bool, Vec<CommandBlock>) {
    let mut blocks = Vec::new();
    let mut language = None;
    let mut marker = b'`';
    let mut marker_count = 0;
    let mut block_end = 0;
    let mut text_end = 0;
    let mut content = String::new();
    let mut nonempty = false;
    let mut safe_text = true;
    for (event, range) in Parser::new(source).into_offset_iter() {
        match event {
            Event::Start(Tag::CodeBlock(CodeBlockKind::Fenced(info))) => {
                language = info.split_whitespace().next().and_then(|name| {
                    ["sh", "bash", "shell", "zsh", "fish"]
                        .into_iter()
                        .find(|known| name.eq_ignore_ascii_case(known))
                });
                content.clear();
                nonempty = false;
                safe_text = true;
                block_end = range.end;
                text_end = range.start;
                let opening = source[range].lines().next().unwrap_or("");
                if let Some(start) = opening.find(['`', '~']) {
                    marker = opening.as_bytes()[start];
                    marker_count = opening.as_bytes()[start..]
                        .iter()
                        .take_while(|&&byte| byte == marker)
                        .count();
                } else {
                    language = None;
                }
            }
            Event::Text(text) if language.is_some() => {
                nonempty |= !text.trim().is_empty();
                safe_text &= !text
                    .chars()
                    .any(|ch| ch.is_control() && !matches!(ch, '\n' | '\t' | '\r'));
                text_end = range.end;
                if collect {
                    content.push_str(&text);
                }
            }
            Event::End(TagEnd::CodeBlock) => {
                let Some(name) = language.take() else {
                    continue;
                };
                // An unterminated fence also receives an End event. Only accept
                // a real closing fence after the last parsed code-text range.
                let closing = source
                    .get(text_end..block_end)
                    .unwrap_or("")
                    .trim()
                    .trim_start_matches(|ch: char| ch == '>' || ch.is_whitespace());
                let closed = marker_count >= 3
                    && closing.len() >= marker_count
                    && closing.bytes().all(|byte| byte == marker);
                if closed && nonempty && safe_text {
                    if !collect {
                        return (true, blocks);
                    }
                    blocks.push(CommandBlock {
                        language: name.to_owned(),
                        code: std::mem::take(&mut content),
                    });
                }
            }
            _ => {}
        }
    }
    (!blocks.is_empty(), blocks)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_only_closed_shell_fences_without_rewriting_commands() {
        let source = "Explanation.\n```bash\nprintf '%s\\n' 'hello world'\ncat <<'END'\n$HOME stays literal\nEND\n```\n```python\nprint('not a shell command')\n```\n~~~sh\npwd\n~~~\n```bash\nnot finished";
        let blocks = shell_blocks(source);
        assert_eq!(
            blocks,
            vec![
                CommandBlock {
                    language: "bash".into(),
                    code: "printf '%s\\n' 'hello world'\ncat <<'END'\n$HOME stays literal\nEND\n"
                        .into()
                },
                CommandBlock {
                    language: "sh".into(),
                    code: "pwd\n".into()
                },
            ]
        );
        assert!(has_shell_blocks(source));
    }

    #[test]
    fn no_execution_entry_for_incomplete_empty_untyped_or_control_bearing_code() {
        for source in [
            "```sh\necho pending\n",
            "```sh\n> ```",
            "```sh\n```",
            "```\npwd\n```",
            "    pwd",
            "```sh\nprintf '\u{1b}]52;c;hidden'\n```",
            "````sh\necho pending\n```",
        ] {
            assert!(shell_blocks(source).is_empty(), "{source:?}");
            assert!(!has_shell_blocks(source), "{source:?}");
        }
    }

    #[test]
    fn quoted_and_list_commands_follow_markdown_code_boundaries() {
        let quoted = shell_blocks("> ```shell\n> printf hello\n> ```");
        assert_eq!(quoted[0].code, "printf hello\n");
        let listed = shell_blocks("1. Step\n\n   ```bash\n   pwd\n   ```");
        assert_eq!(listed[0].code, "pwd\n");
        assert!(has_shell_blocks("```BASH\nprintf hello\n```"));
    }
}
