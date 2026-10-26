use super::{get_value, value_text, Value, ValueKind, WorkspaceSetting};

pub(super) fn multiline(setting: &WorkspaceSetting) -> bool {
    matches!(setting.kind, ValueKind::Lines | ValueKind::Structured)
}

fn escape(text: &str) -> String {
    text.replace('\\', "\\\\")
        .replace('\n', "\\n")
        .replace('\r', "\\r")
        .replace('\t', "\\t")
}

fn unescape(text: &str) -> Result<String, String> {
    let mut chars = text.chars();
    let mut result = String::new();
    while let Some(character) = chars.next() {
        result.push(if character == '\\' {
            match chars.next() {
                Some('\\') => '\\',
                Some('n') => '\n',
                Some('r') => '\r',
                Some('t') => '\t',
                _ => return Err("Use \\\\, \\n, \\r or \\t for escaped text".into()),
            }
        } else {
            character
        });
    }
    Ok(result)
}

pub(super) fn display(config: &Value, setting: &WorkspaceSetting) -> Result<String, String> {
    let value = get_value(config, setting.id);
    match setting.kind {
        ValueKind::Lines => Ok(value
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
            .map(|text| {
                if setting.id.starts_with("commands.") {
                    text.to_string()
                } else if text.is_empty() {
                    "\\e".into()
                } else {
                    escape(text)
                }
            })
            .collect::<Vec<_>>()
            .join("\n")),
        ValueKind::Structured if setting.id == "mcp_servers" => {
            let servers: Vec<crate::config::McpServerConfig> =
                serde_json::from_value(value.cloned().unwrap_or_else(|| serde_json::json!([])))
                    .map_err(|error| error.to_string())?;
            Ok(servers
                .iter()
                .map(|server| {
                    let mut lines = vec![
                        format!("name: {}", escape(&server.name)),
                        format!("transport: {}", escape(&server.transport)),
                    ];
                    if let Some(command) = &server.command {
                        lines.push(format!("command: {}", escape(command)));
                    }
                    if let Some(args) = &server.args {
                        // Single quotes preserve each literal argv token, including empty tokens.
                        let args = args
                            .iter()
                            .map(|arg| format!("'{}'", arg.replace('\'', "'\\''")))
                            .collect::<Vec<_>>()
                            .join(" ");
                        lines.push(format!("args: {}", escape(&args)));
                    }
                    if let Some(url) = &server.url {
                        lines.push(format!("url: {}", escape(url)));
                    }
                    lines.push(format!("enabled: {}", server.enabled));
                    lines.join("\n")
                })
                .collect::<Vec<_>>()
                .join("\n\n"))
        }
        ValueKind::Structured if setting.id == "tui.status_widgets" => Ok(value
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .map(|widget| {
                format!(
                    "{} {} {}",
                    widget["id"].as_str().unwrap_or_default(),
                    widget["row"],
                    widget["area"]
                )
            })
            .collect::<Vec<_>>()
            .join("\n")),
        ValueKind::Structured => Ok(value
            .and_then(Value::as_object)
            .into_iter()
            .flatten()
            .map(|(action, chord)| format!("{action}={}", chord.as_str().unwrap_or_default()))
            .collect::<Vec<_>>()
            .join("\n")),
        _ => Ok(value_text(value)),
    }
}

pub(super) fn serialized(setting: &WorkspaceSetting, text: &str) -> Result<String, String> {
    let value = match setting.kind {
        ValueKind::Lines if setting.id.starts_with("commands.") => Value::from(
            text.lines()
                .map(str::trim)
                .filter(|line| !line.is_empty())
                .collect::<Vec<_>>(),
        ),
        ValueKind::Lines => Value::from(
            text.split('\n')
                .filter(|line| !line.is_empty())
                .map(|line| {
                    if line == "\\e" {
                        Ok(String::new())
                    } else {
                        unescape(line)
                    }
                })
                .collect::<Result<Vec<_>, String>>()?,
        ),
        ValueKind::Structured if setting.id == "mcp_servers" => {
            let mut servers = Vec::new();
            let mut fields = serde_json::Map::new();
            for line in text.lines().chain(std::iter::once("")) {
                if line.trim().is_empty() {
                    if !fields.is_empty() {
                        servers.push(Value::Object(std::mem::take(&mut fields)));
                    }
                    continue;
                }
                let (key, value) = line
                    .split_once(':')
                    .ok_or("Use labeled fields: name: value")?;
                let key = key.trim();
                if !["name", "transport", "command", "args", "url", "enabled"].contains(&key) {
                    return Err(format!("Unknown MCP field: {key}"));
                }
                let value = unescape(value.strip_prefix(' ').unwrap_or(value))?;
                let value = match key {
                    "enabled" => {
                        Value::Bool(value.parse().map_err(|_| "enabled must be true or false")?)
                    }
                    "args" => Value::from(shlex::split(&value).ok_or("Unclosed quoting in args")?),
                    _ => Value::String(value),
                };
                if fields.insert(key.into(), value).is_some() {
                    return Err(format!(
                        "Duplicate MCP field: {key}; separate servers with a blank line"
                    ));
                }
            }
            Value::Array(servers)
        }
        ValueKind::Structured if setting.id == "tui.status_widgets" => {
            let mut widgets = Vec::new();
            for line in text.lines().map(str::trim).filter(|line| !line.is_empty()) {
                let fields: Vec<_> = line.split_whitespace().collect();
                if fields.len() != 3 {
                    return Err("Use one widget per line: provider 1 1".into());
                }
                let row: u64 = fields[1].parse().map_err(|_| "Widget row must be 1 or 2")?;
                let area: u64 = fields[2].parse().map_err(|_| "Widget area must be 1–6")?;
                widgets.push(serde_json::json!({"id": fields[0], "row": row, "area": area}));
            }
            Value::Array(widgets)
        }
        ValueKind::Structured => {
            let mut bindings = serde_json::Map::new();
            for line in text.lines().map(str::trim).filter(|line| !line.is_empty()) {
                let (action, chord) = line
                    .split_once('=')
                    .ok_or("Use one mapping per line: new_chat=alt+n")?;
                if bindings
                    .insert(action.trim().into(), Value::String(chord.trim().into()))
                    .is_some()
                {
                    return Err(format!("Duplicate action: {}", action.trim()));
                }
            }
            Value::Object(bindings)
        }
        _ => return Ok(text.to_string()),
    };
    serde_json::to_string(&value).map_err(|error| error.to_string())
}

pub(super) fn move_line(text: &str, cursor: usize, down: bool) -> usize {
    let start = text[..cursor].rfind('\n').map_or(0, |index| index + 1);
    let column = text[start..cursor].chars().count();
    let target = if down {
        match text[cursor..].find('\n') {
            Some(index) => cursor + index + 1,
            None => return cursor,
        }
    } else {
        if start == 0 {
            return cursor;
        }
        text[..start - 1].rfind('\n').map_or(0, |index| index + 1)
    };
    let line = text[target..].split('\n').next().unwrap_or_default();
    target
        + line
            .char_indices()
            .nth(column)
            .map_or(line.len(), |(index, _)| index)
}
