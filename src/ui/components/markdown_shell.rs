use std::{
    borrow::Cow,
    cell::RefCell,
    collections::VecDeque,
    ops::{ControlFlow, Range},
    rc::Rc,
};

use tree_sitter::{Node, ParseOptions, Parser};

const MAX_SOURCE_BYTES: usize = 65_536;
const CACHE_ENTRIES: usize = 16;

struct CachedCommands {
    source: String,
    fish: bool,
    ranges: Rc<[Range<usize>]>,
}

#[derive(Default)]
struct Highlighter {
    parser: Parser,
    fish: Option<bool>,
    entries: VecDeque<CachedCommands>,
}

thread_local! {
    // Cache syntax, not colors or wrapping, just like the diagram/chart renderers.
    static HIGHLIGHTER: RefCell<Highlighter> = RefCell::new(Highlighter::default());
}

pub(super) fn command_ranges(
    original: &str,
    lines: &[Cow<'_, str>],
    language: &str,
) -> Option<Rc<[Range<usize>]>> {
    if !["sh", "bash", "shell", "zsh", "fish"]
        .iter()
        .any(|name| language.eq_ignore_ascii_case(name))
        || original.len() > MAX_SOURCE_BYTES
    {
        return None;
    }
    let source = if lines.iter().any(|line| matches!(line, Cow::Owned(_))) {
        Cow::Owned(lines.join("\n"))
    } else {
        Cow::Borrowed(original)
    };
    if source.len() > MAX_SOURCE_BYTES {
        return None;
    }
    let fish = language.eq_ignore_ascii_case("fish");
    HIGHLIGHTER.with(|highlighter| {
        let mut highlighter = highlighter.borrow_mut();
        if let Some(index) = highlighter
            .entries
            .iter()
            .position(|entry| entry.fish == fish && entry.source == source.as_ref())
        {
            if let Some(entry) = highlighter.entries.remove(index) {
                let ranges = Rc::clone(&entry.ranges);
                highlighter.entries.push_front(entry);
                return Some(ranges);
            }
        }
        let ranges: Rc<[Range<usize>]> = highlighter.parse(&source, fish).into();
        highlighter.entries.truncate(CACHE_ENTRIES - 1);
        highlighter.entries.push_front(CachedCommands {
            source: source.into_owned(),
            fish,
            ranges: Rc::clone(&ranges),
        });
        Some(ranges)
    })
}

impl Highlighter {
    fn parse(&mut self, source: &str, fish: bool) -> Vec<Range<usize>> {
        if self.fish != Some(fish) {
            let language = if fish {
                tree_sitter_fish::language()
            } else {
                tree_sitter_bash::LANGUAGE.into()
            };
            if self.parser.set_language(&language).is_err() {
                return Vec::new();
            }
            self.fish = Some(fish);
        }
        // Highlighting is optional presentation, never an execution/approval gate.
        // Bound parser work on incomplete or adversarial model output.
        let mut checks = 0;
        let mut progress = |_: &tree_sitter::ParseState| {
            checks += 1;
            if checks > 1_000 {
                ControlFlow::Break(())
            } else {
                ControlFlow::Continue(())
            }
        };
        self.parser.reset();
        let Some(tree) = self.parser.parse_with_options(
            &mut |offset, _| source.as_bytes().get(offset..).unwrap_or_default(),
            None,
            Some(ParseOptions::new().progress_callback(&mut progress)),
        ) else {
            return Vec::new();
        };
        let mut ranges = Vec::new();
        let mut cursor = tree.walk();
        'walk: loop {
            let node = cursor.node();
            match node.kind() {
                "command" if !is_annotation(node, source) => {
                    let mut children = node.walk();
                    for child in node.named_children(&mut children) {
                        if !matches!(
                            child.kind(),
                            "comment" | "heredoc_redirect" | "heredoc_body"
                        ) {
                            ranges.push(child.byte_range());
                        }
                    }
                }
                "declaration_command"
                | "unset_command"
                | "variable_assignment"
                | "test_command"
                | "file_redirect"
                | "stream_redirect" => ranges.push(node.byte_range()),
                "heredoc_redirect" => {
                    let end = source[node.byte_range()]
                        .find('\n')
                        .map_or(node.end_byte(), |offset| node.start_byte() + offset);
                    ranges.push(node.start_byte()..end);
                }
                _ => {}
            }
            if cursor.goto_first_child() {
                continue;
            }
            loop {
                if cursor.goto_next_sibling() {
                    break;
                }
                if !cursor.goto_parent() {
                    break 'walk;
                }
            }
        }
        ranges.sort_unstable_by_key(|range| range.start);
        let mut used = 0;
        for index in 0..ranges.len() {
            if used > 0
                && (ranges[index].start <= ranges[used - 1].end
                    || source[ranges[used - 1].end..ranges[index].start]
                        .chars()
                        .all(char::is_whitespace))
            {
                ranges[used - 1].end = ranges[used - 1].end.max(ranges[index].end);
            } else {
                ranges.swap(used, index);
                used += 1;
            }
        }
        ranges.truncate(used);
        ranges
    }
}

fn is_annotation(node: Node<'_>, source: &str) -> bool {
    let Some(name) = node.child_by_field_name("name") else {
        return false;
    };
    let name = &source[name.byte_range()];
    if !matches!(name, "printf" | "echo")
        || node.child_by_field_name("redirect").is_some()
        || node.parent().is_some_and(|parent| {
            matches!(
                parent.kind(),
                "redirected_statement" | "redirect_statement" | "pipeline" | "pipe"
            )
        })
    {
        return false;
    }
    let mut arguments = [""; 4];
    let mut count = 0;
    let mut cursor = node.walk();
    for argument in node.children_by_field_name("argument", &mut cursor) {
        if count == arguments.len() {
            return false;
        }
        let text = &source[argument.byte_range()];
        // Never quiet a label that itself executes a command substitution.
        if text.contains("$(") || text.contains('`') || text.contains('(') {
            return false;
        }
        arguments[count] = unquote(text);
        count += 1;
    }
    if name == "echo" {
        return count == 1 && is_heading(arguments[0]);
    }
    let format = arguments[0];
    (count == 2 && matches!(format, "%s\\n" | "\\n%s\\n") && is_heading(arguments[1]))
        || (count == 1 && is_heading(format.trim_end_matches("\\n")))
        || (count >= 2
            && arguments[..count]
                .iter()
                .any(|text| text.contains("exit status:") || text.contains("exit: "))
            && arguments[..count]
                .iter()
                .all(|text| !text.contains('$') || matches!(*text, "$?" | "$status")))
}

fn unquote(text: &str) -> &str {
    text.strip_prefix('\'')
        .and_then(|text| text.strip_suffix('\''))
        .or_else(|| {
            text.strip_prefix('"')
                .and_then(|text| text.strip_suffix('"'))
        })
        .unwrap_or(text)
}

fn is_heading(text: &str) -> bool {
    !text.contains('$')
        && ((text.starts_with("--- ") && text.ends_with(" ---"))
            || (text.starts_with("=== ") && text.ends_with(" ==="))
            || text.starts_with("# --- "))
}
