pub fn strip_ansi(value: &str) -> String {
    if !value.contains('\u{001b}') && !value.contains('\u{009b}') {
        return value.to_string();
    }

    let mut result = String::new();
    let mut i = 0;
    while i < value.len() {
        if let Some(length) = ansi_sequence_len(value, i) {
            i += length;
            continue;
        }

        let ch = value[i..].chars().next().expect("valid string boundary");
        result.push(ch);
        i += ch.len_utf8();
    }

    result
}

fn ansi_sequence_len(value: &str, pos: usize) -> Option<usize> {
    if pos >= value.len() || !value.is_char_boundary(pos) {
        return None;
    }

    let bytes = value.as_bytes();
    match bytes[pos] {
        0x1b => match *bytes.get(pos + 1)? {
            b']' => osc_sequence_len(bytes, pos),
            b'[' | b'(' | b')' | b'#' | b';' | b'?' => csi_sequence_len(bytes, pos),
            _ => None,
        },
        0x9b => csi_sequence_len(bytes, pos),
        _ => None,
    }
}

fn osc_sequence_len(bytes: &[u8], pos: usize) -> Option<usize> {
    let mut end = pos + 2;
    while end < bytes.len() {
        if bytes[end] == 0x07 {
            return Some(end + 1 - pos);
        }
        if bytes[end] == 0x1b && bytes.get(end + 1) == Some(&b'\\') {
            return Some(end + 2 - pos);
        }
        end += 1;
    }
    None
}

fn csi_sequence_len(bytes: &[u8], pos: usize) -> Option<usize> {
    let mut end = if bytes[pos] == 0x9b { pos + 1 } else { pos + 2 };
    while end < bytes.len() {
        let byte = bytes[end];
        if (0x40..=0x7e).contains(&byte) {
            return Some(end + 1 - pos);
        }
        end += 1;
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strips_csi_and_osc_sequences() {
        assert_eq!(strip_ansi("\x1b[31mred\x1b[0m"), "red");
        assert_eq!(
            strip_ansi("\x1b]8;;https://example.com\x07link\x1b]8;;\x07"),
            "link"
        );
    }

    #[test]
    fn preserves_unsupported_escape_sequences_like_pi_regex() {
        assert_eq!(strip_ansi("\x1bXplain"), "\x1bXplain");
    }

    #[test]
    fn leaves_plain_text_unchanged() {
        assert_eq!(strip_ansi("plain"), "plain");
    }
}
