use super::CodexTransportError;

pub(super) struct SseDecoder {
    pending: Vec<u8>,
    data: String,
    event_bytes: usize,
    has_data: bool,
    first_line: bool,
    max_event_bytes: usize,
}

impl SseDecoder {
    pub(super) fn new(max_event_bytes: usize) -> Self {
        Self {
            pending: Vec::new(),
            data: String::new(),
            event_bytes: 0,
            has_data: false,
            first_line: true,
            max_event_bytes,
        }
    }

    pub(super) fn push<F>(
        &mut self,
        bytes: &[u8],
        on_data: &mut F,
    ) -> Result<bool, CodexTransportError>
    where
        F: FnMut(&str) -> Result<bool, CodexTransportError>,
    {
        self.pending.extend_from_slice(bytes);
        self.consume_lines(false, on_data)
    }

    pub(super) fn finish<F>(&mut self, on_data: &mut F) -> Result<(), CodexTransportError>
    where
        F: FnMut(&str) -> Result<bool, CodexTransportError>,
    {
        if !self.consume_lines(true, on_data)? {
            return Ok(());
        }
        if self.event_bytes == 0 && self.pending.is_empty() {
            Ok(())
        } else {
            Err(CodexTransportError::UnterminatedSse)
        }
    }

    fn consume_lines<F>(
        &mut self,
        allow_terminal_cr: bool,
        on_data: &mut F,
    ) -> Result<bool, CodexTransportError>
    where
        F: FnMut(&str) -> Result<bool, CodexTransportError>,
    {
        while let Some((line_end, consumed)) = next_line(&self.pending, allow_terminal_cr) {
            let line = self.pending[..line_end].to_vec();
            self.pending.drain(..consumed);
            self.event_bytes = self
                .event_bytes
                .checked_add(consumed)
                .ok_or(CodexTransportError::SseEventTooLarge)?;
            if self.event_bytes > self.max_event_bytes {
                return Err(CodexTransportError::SseEventTooLarge);
            }
            let line =
                std::str::from_utf8(&line).map_err(|_| CodexTransportError::InvalidSseUtf8)?;
            let line = if self.first_line {
                self.first_line = false;
                line.strip_prefix('\u{feff}').unwrap_or(line)
            } else {
                line
            };
            if line.is_empty() {
                let continue_reading = if self.has_data {
                    on_data(&self.data)?
                } else {
                    true
                };
                self.reset_event();
                if !continue_reading {
                    return Ok(false);
                }
                continue;
            }
            let (field, value) = line.split_once(':').unwrap_or((line, ""));
            if field == "data" {
                let value = value.strip_prefix(' ').unwrap_or(value);
                if self.has_data {
                    self.data.push('\n');
                }
                self.data.push_str(value);
                self.has_data = true;
            }
        }
        if self
            .event_bytes
            .checked_add(self.pending.len())
            .ok_or(CodexTransportError::SseEventTooLarge)?
            > self.max_event_bytes
        {
            return Err(CodexTransportError::SseEventTooLarge);
        }
        Ok(true)
    }

    fn reset_event(&mut self) {
        self.data.clear();
        self.event_bytes = 0;
        self.has_data = false;
    }
}

fn next_line(bytes: &[u8], allow_terminal_cr: bool) -> Option<(usize, usize)> {
    let line_end = bytes
        .iter()
        .position(|byte| matches!(byte, b'\n' | b'\r'))?;
    if bytes[line_end] == b'\r' && line_end + 1 == bytes.len() && !allow_terminal_cr {
        return None;
    }
    let consumed = if bytes[line_end] == b'\r' && bytes.get(line_end + 1) == Some(&b'\n') {
        line_end + 2
    } else {
        line_end + 1
    };
    Some((line_end, consumed))
}
