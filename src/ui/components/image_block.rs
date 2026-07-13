use std::path::{Path, PathBuf};

use image::DynamicImage;
use ratatui::{
    Frame,
    layout::{Rect, Size},
};
use ratatui_image::{
    Resize, StatefulImage,
    picker::{Picker, ProtocolType},
    protocol::StatefulProtocol,
    sliced::{SignedPosition, SlicedImage, SlicedProtocol},
};

use crate::ui::components::terminal_capabilities::TerminalCapabilities;

pub struct ImageBlockState {
    image: DynamicImage,
    picker: Picker,
    stateful: Option<StatefulProtocol>,
    sliced: Option<(Size, SlicedProtocol)>,
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
        let image = image::ImageReader::open(path).ok()?.decode().ok()?;
        let picker = picker_for(image_protocol, caps)?;
        Some(Self {
            image,
            picker,
            stateful: None,
            sliced: None,
        })
    }

    pub fn render(&mut self, f: &mut Frame, area: Rect) {
        if self.stateful.is_none() {
            self.stateful = Some(self.picker.new_resize_protocol(self.image.clone()));
        }
        let Some(stateful) = self.stateful.as_mut() else {
            return;
        };
        let widget = StatefulImage::new().resize(Resize::Fit(None));
        f.render_stateful_widget(widget, area, stateful);
    }

    pub fn render_sliced(
        &mut self,
        f: &mut Frame,
        area: Rect,
        size: Size,
        position: SignedPosition,
    ) {
        if area.width == 0 || area.height == 0 {
            return;
        }
        let needs_rebuild = self
            .sliced
            .as_ref()
            .map(|(cached_size, _)| *cached_size != size)
            .unwrap_or(true);
        if needs_rebuild {
            self.sliced = SlicedProtocol::new_with_resize(
                &self.picker,
                self.image.clone(),
                size,
                Resize::Fit(None),
            )
            .ok()
            .map(|protocol| (size, protocol));
        }
        let Some((_, protocol)) = self.sliced.as_ref() else {
            return;
        };
        f.render_widget(SlicedImage::new(protocol, position), area);
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
    if let Some(path) = trimmed.strip_prefix("~/") {
        return dirs::home_dir().map(|home| home.join(path));
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
    let mut picker = crate::ui::components::terminal_capabilities::terminal_picker();
    if !caps.kitty_graphics && image_protocol.eq_ignore_ascii_case("auto") {
        picker.set_protocol_type(ProtocolType::Halfblocks);
    }
    match image_protocol.to_ascii_lowercase().as_str() {
        "auto" => {}
        "halfblocks" => picker.set_protocol_type(ProtocolType::Halfblocks),
        "sixel" => picker.set_protocol_type(ProtocolType::Sixel),
        "kitty" => picker.set_protocol_type(ProtocolType::Kitty),
        "iterm2" => picker.set_protocol_type(ProtocolType::Iterm2),
        _ => {}
    }
    Some(picker)
}

#[cfg(test)]
mod tests {
    use super::{ImageBlockState, picker_for, resolve_local_path};
    use crate::ui::components::terminal_capabilities::{TerminalCapabilities, TerminalKind};
    use image::{DynamicImage, Rgba, RgbaImage};
    use ratatui::{
        Terminal,
        backend::TestBackend,
        layout::{Rect, Size},
    };
    use ratatui_image::picker::ProtocolType;
    use ratatui_image::sliced::SignedPosition;

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
        assert_eq!(picker.protocol_type(), ProtocolType::Kitty);
    }

    #[test]
    fn local_png_encodes_a_kitty_image_block() {
        // Given
        let path =
            std::env::temp_dir().join(format!("tcui-image-block-{}.png", rand::random::<u64>()));
        DynamicImage::ImageRgba8(RgbaImage::from_pixel(4, 4, Rgba([255, 0, 0, 255])))
            .save(&path)
            .expect("save test png");
        let caps = TerminalCapabilities {
            terminal: TerminalKind::Kitty,
            multiplexer: None,
            kitty_graphics: true,
            kitty_text_sizing: true,
            tmux_passthrough: false,
        };
        let mut state =
            ImageBlockState::from_source(path.to_str().expect("utf-8 path"), "kitty", caps)
                .expect("image state");
        let mut terminal = Terminal::new(TestBackend::new(10, 5)).expect("test terminal");

        // When
        terminal
            .draw(|frame| state.render(frame, Rect::new(0, 0, 10, 5)))
            .expect("render image");

        // Then
        assert!(
            terminal
                .backend()
                .buffer()
                .content
                .iter()
                .any(|cell| cell.symbol().contains("\u{1b}_G"))
        );
        std::fs::remove_file(path).expect("remove test png");
    }

    #[test]
    fn sliced_render_handles_top_clipping() {
        let path =
            std::env::temp_dir().join(format!("tcui-sliced-image-{}.png", rand::random::<u64>()));
        DynamicImage::ImageRgba8(RgbaImage::from_pixel(4, 4, Rgba([255, 0, 0, 255])))
            .save(&path)
            .expect("save test png");
        let caps = TerminalCapabilities {
            terminal: TerminalKind::Kitty,
            multiplexer: None,
            kitty_graphics: true,
            kitty_text_sizing: true,
            tmux_passthrough: false,
        };
        let mut state =
            ImageBlockState::from_source(path.to_str().expect("utf-8 path"), "kitty", caps)
                .expect("image state");
        let mut terminal = Terminal::new(TestBackend::new(10, 4)).expect("test terminal");

        terminal
            .draw(|frame| {
                state.render_sliced(
                    frame,
                    Rect::new(0, 0, 10, 4),
                    Size::new(10, 6),
                    SignedPosition::from((0, -2)),
                );
            })
            .expect("render sliced image");

        assert!(state.sliced.is_some());
        std::fs::remove_file(path).expect("remove test png");
    }

    #[test]
    fn image_source_expands_home_directory() {
        let resolved = resolve_local_path("~/Pictures/example.png").expect("resolved home path");

        assert_eq!(
            resolved,
            dirs::home_dir()
                .expect("home directory")
                .join("Pictures/example.png")
        );
    }
}
