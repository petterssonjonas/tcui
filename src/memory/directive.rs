const OPEN_TAG: &str = "<tcui:remember>";
const CLOSE_TAG: &str = "</tcui:remember>";
const MAX_MEMORY_CHARS: usize = 500;

#[derive(Debug, Default, PartialEq, Eq)]
pub(crate) struct FilterResult {
    pub(crate) visible: String,
    pub(crate) memory: Option<String>,
}

#[derive(Debug, Default)]
pub(crate) struct RememberFilter {
    pending_line: String,
    captured: String,
    memory: Option<String>,
    capturing: bool,
    in_fence: bool,
}

impl RememberFilter {
    pub(crate) fn push(&mut self, chunk: &str) -> String {
        self.pending_line.push_str(chunk);
        let mut visible = String::new();
        while let Some(end) = self.pending_line.find('\n') {
            let line = self.pending_line.drain(..=end).collect::<String>();
            self.process_line(&line, &mut visible);
        }
        self.flush_streamable_text(&mut visible);
        visible
    }

    pub(crate) fn finish(mut self) -> FilterResult {
        let mut visible = String::new();
        if !self.pending_line.is_empty() {
            let line = std::mem::take(&mut self.pending_line);
            self.process_line(&line, &mut visible);
        }
        FilterResult {
            visible,
            memory: self.memory,
        }
    }

    fn process_line(&mut self, line: &str, visible: &mut String) {
        let trimmed = line.trim_start();
        let fence_line = trimmed.starts_with("```") || trimmed.starts_with("~~~");
        if !self.capturing && (self.in_fence || fence_line) {
            visible.push_str(line);
            if fence_line {
                self.in_fence = !self.in_fence;
            }
            return;
        }
        self.process_unfenced(line, visible);
    }

    fn process_unfenced(&mut self, mut text: &str, visible: &mut String) {
        loop {
            if self.capturing {
                let Some(end) = text.find(CLOSE_TAG) else {
                    self.captured.push_str(text);
                    return;
                };
                self.captured.push_str(&text[..end]);
                if self.memory.is_none() {
                    let value = self
                        .captured
                        .trim()
                        .chars()
                        .take(MAX_MEMORY_CHARS)
                        .collect::<String>();
                    if !value.is_empty() {
                        self.memory = Some(value);
                    }
                }
                self.captured.clear();
                self.capturing = false;
                text = &text[end + CLOSE_TAG.len()..];
                continue;
            }

            let Some(start) = text.find(OPEN_TAG) else {
                visible.push_str(text);
                return;
            };
            visible.push_str(&text[..start]);
            self.capturing = true;
            text = &text[start + OPEN_TAG.len()..];
        }
    }

    fn flush_streamable_text(&mut self, visible: &mut String) {
        if self.in_fence || might_be_fence_line(&self.pending_line) {
            return;
        }

        let retained = partial_tag_len(&self.pending_line, OPEN_TAG)
            .max(partial_tag_len(&self.pending_line, CLOSE_TAG));
        let streamable = self.pending_line.len() - retained;
        if streamable == 0 {
            return;
        }

        let text = self.pending_line.drain(..streamable).collect::<String>();
        self.process_unfenced(&text, visible);
    }
}

fn partial_tag_len(text: &str, tag: &str) -> usize {
    (1..tag.len())
        .rev()
        .find(|&length| text.ends_with(&tag[..length]))
        .unwrap_or(0)
}

fn might_be_fence_line(line: &str) -> bool {
    let trimmed = line.trim_start();
    trimmed.is_empty()
        || "```".starts_with(trimmed)
        || "~~~".starts_with(trimmed)
        || trimmed.starts_with("```")
        || trimmed.starts_with("~~~")
}

#[cfg(test)]
mod tests {
    use super::RememberFilter;

    #[test]
    fn directive_is_filtered_at_every_stream_boundary() {
        // Given
        let input = "Answer.\n<tcui:remember>User prefers Rust.</tcui:remember>\n";

        for boundary in 0..=input.len() {
            let mut filter = RememberFilter::default();

            // When
            let mut emitted = filter.push(&input[..boundary]);
            emitted.push_str(&filter.push(&input[boundary..]));
            let result = filter.finish();
            emitted.push_str(&result.visible);

            // Then
            assert_eq!(emitted, "Answer.\n\n", "boundary {boundary}");
            assert_eq!(result.memory.as_deref(), Some("User prefers Rust."));
        }
    }

    #[test]
    fn ordinary_text_streams_before_a_newline() {
        // Given
        let mut filter = RememberFilter::default();

        // When / Then
        assert_eq!(filter.push("Visible response"), "Visible response");
        assert_eq!(filter.finish().visible, "");
    }

    #[test]
    fn partial_directive_is_held_while_prior_text_streams() {
        // Given
        let mut filter = RememberFilter::default();

        // When
        let visible = filter.push("Answer.<tcui:rem");
        let hidden = filter.push("ember>fact</tcui:remember>");
        let result = filter.finish();

        // Then
        assert_eq!(visible, "Answer.");
        assert_eq!(hidden, "");
        assert_eq!(result.visible, "");
        assert_eq!(result.memory.as_deref(), Some("fact"));
    }

    #[test]
    fn directive_inside_code_fence_remains_visible() {
        // Given
        let input =
            "```xml\n<tcui:remember>example secret</tcui:remember>\n```\nVisible response.\n";
        let mut filter = RememberFilter::default();

        // When
        let mut emitted = filter.push(input);
        let result = filter.finish();
        emitted.push_str(&result.visible);

        // Then
        assert_eq!(emitted, input);
        assert_eq!(result.memory, None);
    }

    #[test]
    fn only_first_directive_is_captured_and_all_envelopes_are_hidden() {
        // Given
        let input = concat!(
            "<tcui:remember>first</tcui:remember>\n",
            "<tcui:remember>second</tcui:remember>\n"
        );
        let mut filter = RememberFilter::default();

        // When
        let mut emitted = filter.push(input);
        let result = filter.finish();
        emitted.push_str(&result.visible);

        // Then
        assert_eq!(emitted, "\n\n");
        assert_eq!(result.memory.as_deref(), Some("first"));
    }

    #[test]
    fn captured_memory_is_capped_on_character_boundary() {
        // Given
        let memory = "å".repeat(501);
        let input = format!("<tcui:remember>{memory}</tcui:remember>");
        let mut filter = RememberFilter::default();

        // When
        let _ = filter.push(&input);
        let result = filter.finish();

        // Then
        assert_eq!(
            result
                .memory
                .as_deref()
                .map(str::chars)
                .map(Iterator::count),
            Some(500)
        );
    }

    #[test]
    fn unterminated_directive_is_hidden_at_every_stream_boundary() {
        // Given
        let input = "<tcui:remember>fact";

        for boundary in 0..=input.len() {
            let mut filter = RememberFilter::default();

            // When
            let mut emitted = filter.push(&input[..boundary]);
            emitted.push_str(&filter.push(&input[boundary..]));
            let result = filter.finish();
            emitted.push_str(&result.visible);

            // Then
            assert_eq!(emitted, "", "boundary {boundary}");
            assert_eq!(result.memory, None, "boundary {boundary}");
        }
    }
}
