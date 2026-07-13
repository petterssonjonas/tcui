use std::ops::Range;

use pulldown_cmark::{Event, Options, Parser, Tag, TagEnd};

const MAX_CHUNK_CHARS: usize = 800;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct MemoryChunk {
    pub(crate) start_byte: usize,
    pub(crate) end_byte: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ParsedMemory {
    pub(crate) title: String,
    pub(crate) frontmatter_raw: Option<String>,
    pub(crate) chunks: Vec<MemoryChunk>,
}

pub(crate) fn parse_memory(markdown: &str, fallback_title: &str) -> ParsedMemory {
    let mut options = Options::empty();
    options.insert(Options::ENABLE_YAML_STYLE_METADATA_BLOCKS);
    let parser = Parser::new_ext(markdown, options).into_offset_iter();
    let mut metadata_start = None;
    let mut metadata_range = None;
    let mut block_start = None;
    let mut block_depth = 0usize;
    let mut blocks = Vec::new();
    let mut heading = None;
    let mut in_heading = false;

    for (event, range) in parser {
        match event {
            Event::Start(Tag::MetadataBlock(_)) => metadata_start = Some(range.start),
            Event::End(TagEnd::MetadataBlock(_)) => {
                metadata_range = metadata_start.take().map(|start| start..range.end);
            }
            Event::Start(tag) if is_block_tag(&tag) => {
                if block_depth == 0 {
                    block_start = Some(range.start);
                }
                in_heading = matches!(tag, Tag::Heading { .. });
                block_depth += 1;
            }
            Event::End(tag) if is_block_end(tag) => {
                block_depth = block_depth.saturating_sub(1);
                if block_depth == 0 {
                    if let Some(start) = block_start.take() {
                        blocks.push(start..range.end);
                    }
                    in_heading = false;
                }
            }
            Event::Text(text) if in_heading && heading.is_none() => {
                heading = Some(text.trim().to_string());
            }
            _ => {}
        }
    }

    let frontmatter_raw = metadata_range
        .as_ref()
        .and_then(|range| markdown.get(range.clone()))
        .map(str::to_string);
    let title = frontmatter_raw
        .as_deref()
        .and_then(frontmatter_title)
        .or(heading)
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| fallback_title.to_string());

    ParsedMemory {
        title,
        frontmatter_raw,
        chunks: merge_blocks(markdown, &blocks),
    }
}

fn is_block_tag(tag: &Tag<'_>) -> bool {
    matches!(
        tag,
        Tag::Paragraph
            | Tag::Heading { .. }
            | Tag::BlockQuote(_)
            | Tag::CodeBlock(_)
            | Tag::HtmlBlock
            | Tag::List(_)
            | Tag::FootnoteDefinition(_)
            | Tag::Table(_)
    )
}

const fn is_block_end(tag: TagEnd) -> bool {
    matches!(
        tag,
        TagEnd::Paragraph
            | TagEnd::Heading(_)
            | TagEnd::BlockQuote(_)
            | TagEnd::CodeBlock
            | TagEnd::HtmlBlock
            | TagEnd::List(_)
            | TagEnd::FootnoteDefinition
            | TagEnd::Table
    )
}

fn merge_blocks(markdown: &str, blocks: &[Range<usize>]) -> Vec<MemoryChunk> {
    let mut chunks: Vec<MemoryChunk> = Vec::new();
    for range in blocks {
        if markdown
            .get(range.clone())
            .is_none_or(|text| text.trim().is_empty())
        {
            continue;
        }
        if let Some(last) = chunks.last_mut() {
            let combined = markdown
                .get(last.start_byte..range.end)
                .map(str::chars)
                .map(Iterator::count)
                .unwrap_or(usize::MAX);
            if combined <= MAX_CHUNK_CHARS {
                last.end_byte = range.end;
                continue;
            }
        }
        chunks.push(MemoryChunk {
            start_byte: range.start,
            end_byte: range.end,
        });
    }
    chunks
}

fn frontmatter_title(frontmatter: &str) -> Option<String> {
    frontmatter.lines().find_map(|line| {
        line.trim()
            .strip_prefix("title:")
            .map(str::trim)
            .map(|value| value.trim_matches(['"', '\'']).to_string())
            .filter(|value| !value.is_empty())
    })
}

#[cfg(test)]
mod tests {
    use super::parse_memory;

    #[test]
    fn frontmatter_is_excluded_and_chunks_keep_source_offsets() {
        // Given
        let markdown = "---\ntitle: Preferred editor\nkind: preference\n---\n\n# Editor\n\nUse Neovim.\n\nKeep keybindings concise.\n";

        // When
        let parsed = parse_memory(markdown, "fallback");

        // Then
        assert_eq!(parsed.title, "Preferred editor");
        assert!(
            parsed
                .frontmatter_raw
                .as_deref()
                .is_some_and(|raw| raw.contains("kind: preference"))
        );
        assert!(!parsed.chunks.is_empty());
        assert!(parsed.chunks.iter().all(|chunk| {
            markdown
                .get(chunk.start_byte..chunk.end_byte)
                .is_some_and(|text| !text.contains("kind: preference"))
        }));
        assert!(parsed.chunks.iter().any(|chunk| {
            markdown
                .get(chunk.start_byte..chunk.end_byte)
                .is_some_and(|text| text.contains("Use Neovim."))
        }));
    }
}
