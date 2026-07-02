pub(crate) fn launch_editor(path: &std::path::Path) -> Result<(), String> {
    let editor = std::env::var("EDITOR")
        .ok()
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| "Set $EDITOR to enable Edit.".to_string())?;
    let path_str = shell_escape(path);
    let command_line = format!("{editor} {path_str}");

    if std::env::var_os("TMUX").is_some() {
        std::process::Command::new("tmux")
            .args(["split-window", "-h", &command_line])
            .spawn()
            .map_err(|error| format!("Failed to open editor in tmux: {error}"))?;
        return Ok(());
    }

    let terminal =
        preferred_terminal().ok_or_else(|| "No terminal launcher found for Edit.".to_string())?;
    spawn_terminal_command(&terminal, &command_line)
        .map_err(|error| format!("Failed to open editor: {error}"))?;
    Ok(())
}

pub(crate) fn preferred_terminal() -> Option<String> {
    if let Some(terminal) = std::env::var_os("TERMINAL")
        .and_then(|value| value.into_string().ok())
        .filter(|value| command_exists(value))
    {
        return Some(terminal);
    }
    [
        "x-terminal-emulator",
        "gnome-terminal",
        "ghostty",
        "kitty",
        "wezterm",
        "alacritty",
        "foot",
        "konsole",
        "xfce4-terminal",
    ]
    .into_iter()
    .find(|command| command_exists(command))
    .map(ToString::to_string)
}

pub(crate) fn spawn_terminal_command(terminal: &str, command_line: &str) -> std::io::Result<()> {
    let mut command = std::process::Command::new(terminal);
    match terminal {
        "gnome-terminal" => {
            command.args(["--", "sh", "-lc", command_line]);
        }
        "wezterm" => {
            command.args(["start", "--", "sh", "-lc", command_line]);
        }
        "xfce4-terminal" => {
            command.args(["-x", "sh", "-lc", command_line]);
        }
        "kitty" => {
            command.args(["sh", "-lc", command_line]);
        }
        _ => {
            command.args(["-e", "sh", "-lc", command_line]);
        }
    }
    command.spawn().map(|_| ())
}

pub(crate) fn command_exists(command: &str) -> bool {
    std::env::var_os("PATH")
        .into_iter()
        .flat_map(|value| std::env::split_paths(&value).collect::<Vec<_>>())
        .any(|directory| directory.join(command).is_file())
}

pub(crate) fn shell_escape(path: &std::path::Path) -> String {
    let value = path.display().to_string();
    format!("'{}'", value.replace('\'', "'\"'\"'"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shell_escape_wraps_path_in_single_quotes() {
        assert_eq!(
            shell_escape(std::path::Path::new("/tmp/file.txt")),
            "'/tmp/file.txt'"
        );
    }

    #[test]
    fn shell_escape_escapes_embedded_single_quotes() {
        assert_eq!(
            shell_escape(std::path::Path::new("/tmp/file's.txt")),
            "'/tmp/file'\"'\"'s.txt'"
        );
    }

    #[test]
    fn command_exists_finds_known_commands() {
        assert!(command_exists("cargo"));
        assert!(command_exists("sh"));
    }

    #[test]
    fn command_exists_returns_false_for_nonsense() {
        assert!(!command_exists("definitely-not-a-real-command-12345"));
    }
}
