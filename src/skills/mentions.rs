use std::collections::HashSet;

use pulldown_cmark::{Event, Parser, Tag, TagEnd};

/// Find skill invocations in user-authored prose without changing the prompt.
pub fn mentions(text: &str) -> Vec<String> {
    let bytes = text.as_bytes();
    let command_start = text.len() - text.trim_start().len();
    let mut found = Vec::new();
    let mut seen = HashSet::new();
    let mut excluded_depth = 0;
    let mut quoted_until = 0;

    for (event, range) in Parser::new(text).into_offset_iter() {
        match event {
            Event::Start(Tag::CodeBlock(_) | Tag::BlockQuote(_)) => {
                excluded_depth += 1;
            }
            Event::End(TagEnd::CodeBlock | TagEnd::BlockQuote(_)) => {
                excluded_depth -= 1;
            }
            Event::Text(_) if excluded_depth == 0 => {
                let mut index = range.start.max(quoted_until);
                while index < range.end {
                    let Some(character) = text[index..].chars().next() else {
                        break;
                    };
                    if let Some(end) = quoted_end(text, index, character) {
                        quoted_until = end;
                        index = end;
                        continue;
                    }
                    let start = if character == '@' && !is_email_left_boundary(bytes, index) {
                        index + 1
                    } else if index == command_start
                        && text[index..].starts_with("/skill")
                        && bytes.get(index + 6).is_some_and(u8::is_ascii_whitespace)
                    {
                        let mut start = index + 6;
                        while bytes.get(start).is_some_and(u8::is_ascii_whitespace) {
                            start += 1;
                        }
                        start
                    } else {
                        index += character.len_utf8();
                        continue;
                    };
                    let mut end = start;
                    while end < range.end && is_skill_char(bytes[end]) {
                        end += 1;
                    }
                    if end > start
                        && (character == '@' || bytes.get(end).is_none_or(u8::is_ascii_whitespace))
                    {
                        let name = &text[start..end];
                        if seen.insert(name) {
                            found.push(name.to_owned());
                        }
                    }
                    index = end.max(index + character.len_utf8());
                }
            }
            _ => {}
        }
    }

    found
}

fn quoted_end(text: &str, index: usize, opening: char) -> Option<usize> {
    let closing = match opening {
        '"' => '"',
        '\'' if !text[..index].ends_with(char::is_alphanumeric) => '\'',
        '“' => '”',
        '‘' => '’',
        _ => return None,
    };
    let start = index + opening.len_utf8();
    text[start..]
        .char_indices()
        .find_map(|(offset, character)| {
            let end = start + offset + character.len_utf8();
            (character == closing
                && (closing != '\'' || !text[end..].starts_with(char::is_alphanumeric)))
            .then_some(end)
        })
}

const fn is_skill_char(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_'
}

fn is_email_left_boundary(bytes: &[u8], index: usize) -> bool {
    index
        .checked_sub(1)
        .and_then(|left| bytes.get(left))
        .is_some_and(|byte| is_skill_char(*byte) || matches!(byte, b'.' | b'%' | b'+'))
}

#[cfg(test)]
mod tests {
    use super::mentions;

    #[test]
    fn mentions_parse_tokens_deduplicate_in_order_and_ignore_emails() {
        // Given
        let text = "Use @websearch, @exa_2, me@example.com, then @websearch and @fire-crawl.";

        // When
        let parsed = mentions(text);

        // Then
        assert_eq!(parsed, ["websearch", "exa_2", "fire-crawl"]);
    }

    #[test]
    fn mentions_find_contextual_memory_in_the_middle_of_a_sentence() {
        assert_eq!(
            mentions("I do not @remember what this machine is for; can @memory or @recall help?"),
            ["remember", "memory", "recall"]
        );
    }

    #[test]
    fn mentions_accept_slash_skill_without_rewriting_the_request() {
        let prompt =
            "/skill memory I do not remember what this machine is for. Also @websearch it.";
        assert_eq!(mentions(prompt), ["memory", "websearch"]);
    }

    #[test]
    fn mentions_deduplicate_slash_and_inline_invocations_in_order() {
        assert_eq!(
            mentions("/skill research Use @websearch and @research, then @websearch."),
            ["research", "websearch"]
        );
    }

    #[test]
    fn mentions_ignore_markdown_code_and_blockquote_examples() {
        let prompt = concat!(
            "```text\n@memory save this\n/skill remember example\n```\n",
            "\n    @remember indented example\n",
            "\n> @recall quoted example\n> /skill memory another example\n",
            "\nRead `@memorize example` and `/skill recall example` without invoking them.",
        );
        assert!(mentions(prompt).is_empty());
    }

    #[test]
    fn mentions_ignore_literal_quoted_examples() {
        assert!(mentions(
            "Explain \"@memory save this\", '@remember that', “@recall facts”, \
             and ‘/skill memory example’."
        )
        .is_empty());
    }

    #[test]
    fn mentions_ignore_quoted_examples_across_markdown_formatting() {
        assert_eq!(
            mentions("Read \"an **@memory** example\"; I don't @remember this machine."),
            ["remember"]
        );
    }

    #[test]
    fn mentions_require_a_leading_slash_command_and_exact_skill_command() {
        assert!(mentions("Read /skill memory example; /skills still opens the menu.").is_empty());
        assert!(mentions("/skills memory example").is_empty());
        assert!(mentions("/skillful memory example").is_empty());
    }
}
