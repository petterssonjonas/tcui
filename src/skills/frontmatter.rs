use std::fs::File;
use std::io::{self, BufRead, BufReader};
use std::path::Path;

const MAX_METADATA_BYTES: usize = 16 * 1024;

#[derive(Debug, Eq, PartialEq)]
pub(crate) struct ParsedMetadata {
    pub(crate) name: String,
    pub(crate) description: String,
}

pub(crate) fn read_metadata(path: &Path, fallback_name: &str) -> io::Result<ParsedMetadata> {
    let file = File::open(path)?;
    let mut reader = BufReader::with_capacity(512, file);
    let mut source = String::new();
    let mut line = String::new();

    if reader.read_line(&mut line)? == 0 || line.trim_end() != "---" {
        return Ok(parse_metadata("", fallback_name));
    }
    source.push_str(&line);
    line.clear();

    while source.len() < MAX_METADATA_BYTES && reader.read_line(&mut line)? != 0 {
        source.push_str(&line);
        let is_end = line.trim_end() == "---";
        line.clear();
        if is_end {
            break;
        }
    }

    Ok(parse_metadata(&source, fallback_name))
}

pub(crate) fn parse_metadata(source: &str, fallback_name: &str) -> ParsedMetadata {
    let mut name = None;
    let mut description = None;
    let mut lines = source.lines();

    if lines.next().map(str::trim) == Some("---") {
        for line in lines {
            if line.trim() == "---" {
                break;
            }
            let Some((key, value)) = line.split_once(':') else {
                continue;
            };
            let value = value.trim().trim_matches(['"', '\'']);
            match key.trim() {
                "name" if !value.is_empty() => name = Some(value.to_owned()),
                "description" if !value.is_empty() => description = Some(value.to_owned()),
                _ => {}
            }
        }
    }

    ParsedMetadata {
        name: name.unwrap_or_else(|| fallback_name.to_owned()),
        description: description.unwrap_or_default(),
    }
}

#[cfg(test)]
mod tests {
    use super::{ParsedMetadata, parse_metadata};

    #[test]
    fn metadata_uses_directory_name_when_frontmatter_name_is_missing() {
        // Given
        let source = "---\ndescription: Search project notes.\n---\n\n# Notes\n";

        // When
        let metadata = parse_metadata(source, "notes");

        // Then
        assert_eq!(
            metadata,
            ParsedMetadata {
                name: "notes".to_owned(),
                description: "Search project notes.".to_owned(),
            }
        );
    }
}
