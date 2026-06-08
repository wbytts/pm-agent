const ESC: &str = "\x1b";
const BRACKETED_PASTE_START: &str = "\x1b[200~";
const BRACKETED_PASTE_END: &str = "\x1b[201~";

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StdinBufferEvent {
    Data(String),
    Paste(String),
}

#[derive(Debug, Clone, Default)]
pub struct StdinBuffer {
    buffer: String,
    paste_mode: bool,
    paste_buffer: String,
    pending_kitty_printable_codepoint: Option<u32>,
}

impl StdinBuffer {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn process(&mut self, data: impl AsRef<[u8]>) -> Vec<StdinBufferEvent> {
        let bytes = data.as_ref();
        let input = if bytes.len() == 1 && bytes[0] > 127 {
            format!("\x1b{}", (bytes[0] - 128) as char)
        } else {
            String::from_utf8_lossy(bytes).to_string()
        };
        let mut events = Vec::new();
        if input.is_empty() && self.buffer.is_empty() {
            self.emit_data(String::new(), &mut events);
            return events;
        }
        self.buffer.push_str(&input);

        if self.paste_mode {
            self.paste_buffer.push_str(&self.buffer);
            self.buffer.clear();
            self.finish_paste_if_complete(&mut events);
            return events;
        }

        if let Some(start_index) = self.buffer.find(BRACKETED_PASTE_START) {
            if start_index > 0 {
                let before = self.buffer[..start_index].to_string();
                let result = extract_complete_sequences(&before);
                for sequence in result.sequences {
                    self.emit_data(sequence, &mut events);
                }
            }
            self.pending_kitty_printable_codepoint = None;
            self.paste_mode = true;
            self.paste_buffer =
                self.buffer[start_index + BRACKETED_PASTE_START.len()..].to_string();
            self.buffer.clear();
            self.finish_paste_if_complete(&mut events);
            return events;
        }

        if is_incomplete_escape_prefix(&self.buffer) {
            return events;
        }

        if is_exact_complete_legacy_sequence(&self.buffer) {
            let sequence = std::mem::take(&mut self.buffer);
            self.emit_data(sequence, &mut events);
            return events;
        }

        let result = extract_complete_sequences(&self.buffer);
        self.buffer = result.remainder;
        for sequence in result.sequences {
            self.emit_data(sequence, &mut events);
        }
        events
    }

    pub fn flush(&mut self) -> Vec<String> {
        if self.buffer.is_empty() {
            return Vec::new();
        }
        let flushed = std::mem::take(&mut self.buffer);
        self.pending_kitty_printable_codepoint = None;
        vec![flushed]
    }

    pub fn clear(&mut self) {
        self.buffer.clear();
        self.paste_mode = false;
        self.paste_buffer.clear();
        self.pending_kitty_printable_codepoint = None;
    }

    pub fn buffer(&self) -> &str {
        &self.buffer
    }

    fn finish_paste_if_complete(&mut self, events: &mut Vec<StdinBufferEvent>) {
        let Some(end_index) = self.paste_buffer.find(BRACKETED_PASTE_END) else {
            return;
        };
        let pasted = self.paste_buffer[..end_index].to_string();
        let remaining = self.paste_buffer[end_index + BRACKETED_PASTE_END.len()..].to_string();
        self.paste_mode = false;
        self.paste_buffer.clear();
        self.pending_kitty_printable_codepoint = None;
        events.push(StdinBufferEvent::Paste(pasted));
        if !remaining.is_empty() {
            events.extend(self.process(remaining));
        }
    }

    fn emit_data(&mut self, sequence: String, events: &mut Vec<StdinBufferEvent>) {
        let raw_codepoint = (sequence.chars().count() == 1)
            .then(|| sequence.chars().next().map(|ch| ch as u32))
            .flatten();
        if raw_codepoint.is_some() && raw_codepoint == self.pending_kitty_printable_codepoint {
            self.pending_kitty_printable_codepoint = None;
            return;
        }
        self.pending_kitty_printable_codepoint =
            parse_unmodified_kitty_printable_codepoint(&sequence);
        events.push(StdinBufferEvent::Data(sequence));
    }
}

fn is_incomplete_escape_prefix(data: &str) -> bool {
    matches!(
        data,
        "\x1b" | "\x1b[" | "\x1b]" | "\x1bO" | "\x1bP" | "\x1b_"
    )
}

fn is_exact_complete_legacy_sequence(data: &str) -> bool {
    matches!(
        data,
        "\x1b[A"
            | "\x1b[B"
            | "\x1b[C"
            | "\x1b[D"
            | "\x1b[H"
            | "\x1b[F"
            | "\x1bOA"
            | "\x1bOB"
            | "\x1bOC"
            | "\x1bOD"
            | "\x1bOH"
            | "\x1bOF"
            | "\x1bOP"
            | "\x1bOQ"
            | "\x1bOR"
            | "\x1bOS"
            | "\x1b[2~"
            | "\x1b[3~"
            | "\x1b[5~"
            | "\x1b[6~"
            | "\x1b[7~"
            | "\x1b[8~"
            | "\x1b[11~"
            | "\x1b[12~"
            | "\x1b[13~"
            | "\x1b[14~"
            | "\x1b[15~"
            | "\x1b[17~"
            | "\x1b[18~"
            | "\x1b[19~"
            | "\x1b[20~"
            | "\x1b[21~"
            | "\x1b[23~"
            | "\x1b[24~"
    )
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ExtractResult {
    sequences: Vec<String>,
    remainder: String,
}

fn extract_complete_sequences(buffer: &str) -> ExtractResult {
    let mut sequences = Vec::new();
    let mut pos = 0;
    while pos < buffer.len() {
        let remaining = &buffer[pos..];
        if remaining.starts_with(ESC) {
            let mut seq_end = 1;
            let mut advanced = false;
            while seq_end <= remaining.len() {
                let candidate = &remaining[..seq_end];
                match complete_sequence_status(candidate) {
                    SequenceStatus::Complete => {
                        if candidate == "\x1b\x1b" {
                            if let Some(next) = remaining.as_bytes().get(seq_end).copied() {
                                if matches!(next, b'[' | b']' | b'O' | b'P' | b'_') {
                                    sequences.push(ESC.to_string());
                                    pos += 1;
                                    advanced = true;
                                    break;
                                }
                            }
                        }
                        sequences.push(candidate.to_string());
                        pos += seq_end;
                        advanced = true;
                        break;
                    }
                    SequenceStatus::Incomplete => seq_end += 1,
                    SequenceStatus::NotEscape => {
                        sequences.push(candidate.to_string());
                        pos += seq_end;
                        advanced = true;
                        break;
                    }
                }
            }
            if !advanced {
                return ExtractResult {
                    sequences,
                    remainder: remaining.to_string(),
                };
            }
        } else if let Some(ch) = remaining.chars().next() {
            sequences.push(ch.to_string());
            pos += ch.len_utf8();
        } else {
            break;
        }
    }
    ExtractResult {
        sequences,
        remainder: String::new(),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SequenceStatus {
    Complete,
    Incomplete,
    NotEscape,
}

fn complete_sequence_status(data: &str) -> SequenceStatus {
    if !data.starts_with(ESC) {
        return SequenceStatus::NotEscape;
    }
    if data.len() == 1 {
        return SequenceStatus::Incomplete;
    }
    if matches!(
        data,
        "\x1b[A"
            | "\x1b[B"
            | "\x1b[C"
            | "\x1b[D"
            | "\x1b[H"
            | "\x1b[F"
            | "\x1bOA"
            | "\x1bOB"
            | "\x1bOC"
            | "\x1bOD"
            | "\x1bOH"
            | "\x1bOF"
            | "\x1bOP"
            | "\x1bOQ"
            | "\x1bOR"
            | "\x1bOS"
    ) {
        return SequenceStatus::Complete;
    }
    let after_esc = &data[1..];
    if after_esc.starts_with('[') {
        if after_esc.starts_with("[M") {
            return if data.len() >= 6 {
                SequenceStatus::Complete
            } else {
                SequenceStatus::Incomplete
            };
        }
        return complete_csi_sequence(data);
    }
    if after_esc.starts_with(']') {
        return complete_osc_sequence(data);
    }
    if after_esc.starts_with('P') {
        return complete_string_terminator_sequence(data, "\x1bP");
    }
    if after_esc.starts_with('_') {
        return complete_string_terminator_sequence(data, "\x1b_");
    }
    if after_esc.starts_with('O') {
        return if after_esc.len() >= 2 {
            SequenceStatus::Complete
        } else {
            SequenceStatus::Incomplete
        };
    }
    if after_esc.chars().count() == 1 {
        return SequenceStatus::Complete;
    }
    SequenceStatus::Complete
}

fn complete_csi_sequence(data: &str) -> SequenceStatus {
    if data.len() < 3 {
        return SequenceStatus::Incomplete;
    }
    let payload = &data[2..];
    let Some(last) = payload.chars().last() else {
        return SequenceStatus::Incomplete;
    };
    let code = last as u32;
    if (0x40..=0x7e).contains(&code) {
        if payload.starts_with('<') {
            let body = &payload[1..payload.len().saturating_sub(1)];
            let valid_mouse = matches!(last, 'M' | 'm')
                && body.split(';').count() == 3
                && body
                    .split(';')
                    .all(|part| part.chars().all(|ch| ch.is_ascii_digit()));
            return if valid_mouse {
                SequenceStatus::Complete
            } else {
                SequenceStatus::Incomplete
            };
        }
        return SequenceStatus::Complete;
    }
    SequenceStatus::Incomplete
}

fn complete_osc_sequence(data: &str) -> SequenceStatus {
    if !data.starts_with("\x1b]") {
        return SequenceStatus::Complete;
    }
    if data.ends_with("\x1b\\") || data.ends_with('\x07') {
        SequenceStatus::Complete
    } else {
        SequenceStatus::Incomplete
    }
}

fn complete_string_terminator_sequence(data: &str, prefix: &str) -> SequenceStatus {
    if !data.starts_with(prefix) {
        return SequenceStatus::Complete;
    }
    if data.ends_with("\x1b\\") {
        SequenceStatus::Complete
    } else {
        SequenceStatus::Incomplete
    }
}

fn parse_unmodified_kitty_printable_codepoint(sequence: &str) -> Option<u32> {
    let body = sequence.strip_prefix("\x1b[")?.strip_suffix('u')?;
    let codepoint = body
        .split([';', ':'])
        .next()
        .and_then(|value| value.parse::<u32>().ok())?;
    (codepoint >= 32).then_some(codepoint)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn buffers_partial_escape_sequences() {
        let mut buffer = StdinBuffer::new();
        assert!(buffer.process("\x1b[").is_empty());
        let events = buffer.process("A");
        assert_eq!(events, vec![StdinBufferEvent::Data("\x1b[A".to_string())]);
    }

    #[test]
    fn emits_bracketed_paste_as_paste_event() {
        let mut buffer = StdinBuffer::new();
        let events = buffer.process("\x1b[200~hello\x1b[201~x");
        assert_eq!(
            events,
            vec![
                StdinBufferEvent::Paste("hello".to_string()),
                StdinBufferEvent::Data("x".to_string())
            ]
        );
    }

    #[test]
    fn dcs_and_apc_sequences_require_st_terminator_while_osc_allows_bel_like_pi() {
        let mut osc = StdinBuffer::new();
        assert_eq!(
            osc.process("\x1b]0;title\x07"),
            vec![StdinBufferEvent::Data("\x1b]0;title\x07".to_string())]
        );

        let mut dcs = StdinBuffer::new();
        assert!(dcs.process("\x1bP>|payload\x07").is_empty());
        assert_eq!(dcs.buffer(), "\x1bP>|payload\x07");
        assert_eq!(
            dcs.process("\x1b\\"),
            vec![StdinBufferEvent::Data(
                "\x1bP>|payload\x07\x1b\\".to_string()
            )]
        );

        let mut apc = StdinBuffer::new();
        assert!(apc.process("\x1b_Gi=1\x07").is_empty());
        assert_eq!(apc.buffer(), "\x1b_Gi=1\x07");
        assert_eq!(
            apc.process("\x1b\\"),
            vec![StdinBufferEvent::Data("\x1b_Gi=1\x07\x1b\\".to_string())]
        );
    }

    #[test]
    fn flush_returns_incomplete_remainder() {
        let mut buffer = StdinBuffer::new();
        assert!(buffer.process("\x1b[").is_empty());
        assert_eq!(buffer.flush(), vec!["\x1b[".to_string()]);
    }
}
