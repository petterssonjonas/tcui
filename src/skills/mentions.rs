use std::collections::HashSet;

pub fn mentions(text: &str) -> Vec<String> {
    let bytes = text.as_bytes();
    let mut found = Vec::new();
    let mut seen = HashSet::new();
    let mut index = 0;

    while index < bytes.len() {
        if bytes[index] != b'@' || is_email_left_boundary(bytes, index) {
            index += 1;
            continue;
        }

        let start = index + 1;
        let mut end = start;
        while end < bytes.len() && is_skill_char(bytes[end]) {
            end += 1;
        }

        if end > start {
            let name = &text[start..end];
            if seen.insert(name) {
                found.push(name.to_owned());
            }
        }
        index = end.max(index + 1);
    }

    found
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
}
