use crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use ratatui::layout::Rect;
use tui_term::vt100::{MouseProtocolEncoding, MouseProtocolMode};

pub(super) fn key_to_bytes(key: KeyEvent) -> Vec<u8> {
    let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
    let alt = key.modifiers.contains(KeyModifiers::ALT);
    let shift = key.modifiers.contains(KeyModifiers::SHIFT);
    match key.code {
        KeyCode::Char(character) => char_to_bytes(character, ctrl, alt, shift),
        KeyCode::Enter => vec![b'\r'],
        KeyCode::Backspace => vec![0x7f],
        KeyCode::Tab => vec![b'\t'],
        KeyCode::BackTab => b"\x1b[Z".to_vec(),
        KeyCode::Esc => vec![0x1b],
        KeyCode::Left => ansi_seq("D", alt),
        KeyCode::Right => ansi_seq("C", alt),
        KeyCode::Up => ansi_seq("A", alt),
        KeyCode::Down => ansi_seq("B", alt),
        KeyCode::Home => ansi_seq("H", alt),
        KeyCode::End => ansi_seq("F", alt),
        KeyCode::PageUp => ansi_seq("5~", alt),
        KeyCode::PageDown => ansi_seq("6~", alt),
        KeyCode::Delete => ansi_seq("3~", alt),
        KeyCode::Insert => ansi_seq("2~", alt),
        KeyCode::F(number) => f_key_bytes(number),
        _ => Vec::new(),
    }
}

fn char_to_bytes(character: char, ctrl: bool, alt: bool, _shift: bool) -> Vec<u8> {
    let mut bytes = Vec::new();
    if alt {
        bytes.push(0x1b);
    }
    if ctrl && character.is_ascii_alphabetic() {
        bytes.push((character as u8) & 0x1f);
    } else if ctrl && matches!(character, ' ' | '@' | '2') {
        bytes.push(0x00);
    } else if ctrl && matches!(character, '[' | '3') {
        bytes.push(0x1b);
    } else if ctrl && matches!(character, '\\' | '4') {
        bytes.push(0x1c);
    } else if ctrl && matches!(character, ']' | '5') {
        bytes.push(0x1d);
    } else if ctrl && matches!(character, '^' | '6') {
        bytes.push(0x1e);
    } else if ctrl && matches!(character, '_' | '7' | '/') {
        bytes.push(0x1f);
    } else if ctrl && matches!(character, '?' | '8') {
        bytes.push(0x7f);
    } else {
        let mut buffer = [0u8; 4];
        bytes.extend_from_slice(character.encode_utf8(&mut buffer).as_bytes());
    }
    bytes
}

fn ansi_seq(code: &str, alt: bool) -> Vec<u8> {
    let prefix = if alt { "\x1b\x1b[" } else { "\x1b[" };
    format!("{prefix}{code}").into_bytes()
}

fn f_key_bytes(number: u8) -> Vec<u8> {
    match number {
        1 => b"\x1bOP".to_vec(),
        2 => b"\x1bOQ".to_vec(),
        3 => b"\x1bOR".to_vec(),
        4 => b"\x1bOS".to_vec(),
        5 => b"\x1b[15~".to_vec(),
        6 => b"\x1b[17~".to_vec(),
        7 => b"\x1b[18~".to_vec(),
        8 => b"\x1b[19~".to_vec(),
        9 => b"\x1b[20~".to_vec(),
        10 => b"\x1b[21~".to_vec(),
        11 => b"\x1b[23~".to_vec(),
        12 => b"\x1b[24~".to_vec(),
        _ => Vec::new(),
    }
}

pub(super) fn mouse_to_bytes(
    event: MouseEvent,
    area: Rect,
    mode: MouseProtocolMode,
    encoding: MouseProtocolEncoding,
) -> Option<Vec<u8>> {
    if mode == MouseProtocolMode::None || !area.contains((event.column, event.row).into()) {
        return None;
    }
    let button_code = |button| match button {
        MouseButton::Left => 0u16,
        MouseButton::Middle => 1,
        MouseButton::Right => 2,
    };
    let release = matches!(event.kind, MouseEventKind::Up(_));
    let mut code = match event.kind {
        MouseEventKind::Down(button) => button_code(button),
        MouseEventKind::Up(button) if mode != MouseProtocolMode::Press => {
            if encoding == MouseProtocolEncoding::Sgr {
                button_code(button)
            } else {
                3
            }
        }
        MouseEventKind::Drag(button)
            if matches!(
                mode,
                MouseProtocolMode::ButtonMotion | MouseProtocolMode::AnyMotion
            ) =>
        {
            button_code(button) | 32
        }
        MouseEventKind::Moved if mode == MouseProtocolMode::AnyMotion => 3 | 32,
        MouseEventKind::ScrollUp if mode != MouseProtocolMode::Press => 64,
        MouseEventKind::ScrollDown if mode != MouseProtocolMode::Press => 65,
        MouseEventKind::ScrollLeft if mode != MouseProtocolMode::Press => 66,
        MouseEventKind::ScrollRight if mode != MouseProtocolMode::Press => 67,
        _ => return Some(Vec::new()),
    };
    if mode != MouseProtocolMode::Press {
        if event.modifiers.contains(KeyModifiers::SHIFT) {
            code |= 4;
        }
        if event.modifiers.contains(KeyModifiers::ALT) {
            code |= 8;
        }
        if event.modifiers.contains(KeyModifiers::CONTROL) {
            code |= 16;
        }
    }
    let x = event.column.saturating_sub(area.x).saturating_add(1);
    let y = event.row.saturating_sub(area.y).saturating_add(1);
    Some(match encoding {
        MouseProtocolEncoding::Sgr => {
            let suffix = if release { 'm' } else { 'M' };
            format!("\x1b[<{code};{x};{y}{suffix}").into_bytes()
        }
        MouseProtocolEncoding::Default => {
            vec![
                0x1b,
                b'[',
                b'M',
                (code + 32) as u8,
                (x.min(223) + 32) as u8,
                (y.min(223) + 32) as u8,
            ]
        }
        MouseProtocolEncoding::Utf8 => {
            let mut bytes = Vec::with_capacity(9);
            bytes.extend_from_slice(b"\x1b[M");
            for value in [code + 32, x.min(2015) + 32, y.min(2015) + 32] {
                if let Some(character) = char::from_u32(u32::from(value)) {
                    let mut buffer = [0; 4];
                    bytes.extend_from_slice(character.encode_utf8(&mut buffer).as_bytes());
                }
            }
            bytes
        }
    })
}
