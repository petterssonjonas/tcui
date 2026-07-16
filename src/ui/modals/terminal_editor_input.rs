use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

pub(super) fn key_to_bytes(key: KeyEvent) -> Vec<u8> {
    let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
    let alt = key.modifiers.contains(KeyModifiers::ALT);
    let shift = key.modifiers.contains(KeyModifiers::SHIFT);
    match key.code {
        KeyCode::Char(character) => char_to_bytes(character, ctrl, alt, shift),
        KeyCode::Enter => vec![b'\r'],
        KeyCode::Backspace => vec![0x7f],
        KeyCode::Tab => vec![b'\t'],
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
    } else if ctrl && character == ' ' {
        bytes.push(0x00);
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
