use std::path::{Path, PathBuf};

use ratatui::{layout::Rect, Frame};
use ratatui_image::{
    picker::{Picker, ProtocolType},
    protocol::StatefulProtocol,
    Resize, StatefulImage,
};

use crate::ui::components::terminal_capabilities::TerminalCapabilities;

pub struct ImageBlockState {
    state: Box<dyn StatefulProtocol>,
}

impl ImageBlockState {
    pub fn from_source(
        source: &str,
        image_protocol: &str,
        caps: TerminalCapabilities,
    ) -> Option<Self> {
        if image_protocol.eq_ignore_ascii_case("off") {
            return None;
        }
        let path = resolve_local_path(source)?;
        let image = image::io::Reader::open(path).ok()?.decode().ok()?;
        let mut picker = picker_for(image_protocol, caps)?;
        let state = picker.new_resize_protocol(image);
        Some(Self { state })
    }

    pub fn render(&mut self, f: &mut Frame, area: Rect) {
        let widget = StatefulImage::new(None).resize(Resize::Fit(None));
        f.render_stateful_widget(widget, area, &mut self.state);
    }
}

pub fn is_local_image_source(source: &str) -> bool {
    resolve_local_path(source).is_some()
}

fn resolve_local_path(source: &str) -> Option<PathBuf> {
    let trimmed = source.trim().trim_matches('<').trim_matches('>');
    if trimmed.starts_with("http://") || trimmed.starts_with("https://") {
        return None;
    }
    if let Some(path) = trimmed.strip_prefix("file://") {
        return Some(PathBuf::from(path));
    }
    let path = Path::new(trimmed);
    if path.is_absolute() || path.exists() {
        return Some(path.to_path_buf());
    }

    std::env::current_dir()
        .ok()
        .map(|cwd| cwd.join(path))
        .filter(|path| path.exists())
}

fn picker_for(image_protocol: &str, caps: TerminalCapabilities) -> Option<Picker> {
    let mut picker = Picker::from_termios()
        .ok()
        .or_else(|| Some(Picker::new((8, 16))))?;
    picker.guess_protocol();
    if !caps.kitty_graphics {
        picker.protocol_type = ProtocolType::Halfblocks;
    }
    match image_protocol.to_ascii_lowercase().as_str() {
        "auto" => {}
        "halfblocks" => picker.protocol_type = ProtocolType::Halfblocks,
        "sixel" => picker.protocol_type = ProtocolType::Sixel,
        "kitty" => picker.protocol_type = ProtocolType::Kitty,
        "iterm2" => picker.protocol_type = ProtocolType::Iterm2,
        _ => {}
    }
    Some(picker)
}

#[cfg(test)]
mod tests {
    use super::picker_for;
    use crate::ui::components::terminal_capabilities::{TerminalCapabilities, TerminalKind};
    use ratatui_image::picker::ProtocolType;

    #[test]
    fn kitty_protocol_remains_selectable() {
        let caps = TerminalCapabilities {
            terminal: TerminalKind::Kitty,
            multiplexer: None,
            kitty_graphics: true,
            kitty_text_sizing: true,
            tmux_passthrough: false,
        };

        let picker = picker_for("kitty", caps).expect("picker");
        assert_eq!(picker.protocol_type, ProtocolType::Kitty);
    }
}
