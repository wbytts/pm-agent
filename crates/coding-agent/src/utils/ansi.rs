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

    let ch = value[pos..].chars().next()?;
    let bytes = value.as_bytes();
    match ch {
        '\u{001b}' => match *bytes.get(pos + 1)? {
            b']' => osc_sequence_len(bytes, pos).or_else(|| csi_like_sequence_len(bytes, pos)),
            _ => csi_like_sequence_len(bytes, pos),
        },
        '\u{009b}' => csi_like_sequence_len(bytes, pos),
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

fn csi_like_sequence_len(bytes: &[u8], pos: usize) -> Option<usize> {
    let mut end = if bytes[pos] == 0x1b {
        pos + 1
    } else {
        pos + '\u{009b}'.len_utf8()
    };
    let intermediate_start = end;
    while end < bytes.len() && is_csi_intermediate(bytes[end]) {
        end += 1;
    }
    let consumed_intermediate = end > intermediate_start;

    if !consumed_intermediate && bytes[pos] == 0x1b {
        if let Some(byte) = bytes.get(end).copied() {
            if is_csi_final(byte) {
                return Some(end + 1 - pos);
            }
        }
    }
    if consumed_intermediate
        && bytes[pos] == 0x1b
        && bytes
            .get(end.saturating_sub(1))
            .is_some_and(|byte| matches!(*byte, b'(' | b')' | b'#'))
    {
        if let Some(byte) = bytes.get(end).copied() {
            if is_csi_final(byte) {
                return Some(end + 1 - pos);
            }
        }
    }

    if end < bytes.len() && bytes[end].is_ascii_digit() {
        let mut digits = 0;
        while end < bytes.len() && bytes[end].is_ascii_digit() && digits < 4 {
            end += 1;
            digits += 1;
        }
        while end < bytes.len() && (bytes[end] == b';' || bytes[end] == b':') {
            end += 1;
            let mut segment_digits = 0;
            while end < bytes.len() && bytes[end].is_ascii_digit() && segment_digits < 4 {
                end += 1;
                segment_digits += 1;
            }
        }
    }

    let byte = *bytes.get(end)?;
    is_csi_final(byte).then_some(end + 1 - pos)
}

fn is_csi_intermediate(byte: u8) -> bool {
    matches!(byte, b'[' | b']' | b'(' | b')' | b'#' | b';' | b'?')
}

fn is_csi_final(byte: u8) -> bool {
    matches!(
        byte,
        b'0'..=b'9'
            | b'A'..=b'P'
            | b'R'..=b'T'
            | b'Z'
            | b'c'
            | b'f'..=b'n'
            | b'q'..=b'u'
            | b'y'
            | b'='
            | b'>'
            | b'<'
            | b'~'
    )
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
        assert_eq!(strip_ansi("x\x1b*y"), "x\x1b*y");
        assert_eq!(strip_ansi("x\x1b+y"), "x\x1b+y");
        assert_eq!(strip_ansi("x\x1b\\y"), "x\x1b\\y");
    }

    #[test]
    fn leaves_plain_text_unchanged() {
        assert_eq!(strip_ansi("plain"), "plain");
    }

    #[test]
    fn strips_common_ansi_sequences_used_in_tool_output_like_pi() {
        let input = "a\x1b[31mred\x1b[0m\x1b]8;;https://example.com\x07link\x1b]8;;\x07z";
        assert_eq!(strip_ansi(input), "aredlinkz");
    }

    #[test]
    fn strips_ris_and_single_byte_esc_sequences_like_pi() {
        assert_eq!(strip_ansi("\x1bcdone"), "done");
        for code in b'g'..=b'm' {
            assert_eq!(
                strip_ansi(&format!("\x1b{}ok", code as char)),
                "ok",
                "ESC {} should strip",
                code as char
            );
        }
        for code in b'r'..=b't' {
            assert_eq!(
                strip_ansi(&format!("\x1b{}ok", code as char)),
                "ok",
                "ESC {} should strip",
                code as char
            );
        }
    }

    #[test]
    fn strips_csi_like_sequences_like_pi_ansi_regex() {
        assert_eq!(strip_ansi("a\x1bPabc\x1b\\z"), "aabc\x1b\\z");
        assert_eq!(strip_ansi("a\x1b^abc\x07z"), "a\x1b^abc\x07z");
        assert_eq!(strip_ansi("a\x1b_abc\u{009c}z"), "a\x1b_abc\u{009c}z");
        assert_eq!(strip_ansi("a\u{0090}abc\u{009c}z"), "a\u{0090}abc\u{009c}z");
        assert_eq!(strip_ansi("a\u{009d}abc\u{009c}z"), "a\u{009d}abc\u{009c}z");
        assert_eq!(strip_ansi("a\u{009b}31mred"), "ared");
    }

    #[test]
    fn matches_pi_strip_ansi_compatibility_inputs() {
        for (input, expected) in [
            ("plain", "plain"),
            ("a\x1b[31mred\x1b[0mz", "aredz"),
            (
                "a\x1b]8;;https://example.com\x07link\x1b]8;;\x07z",
                "alinkz",
            ),
            ("a\x1b]unterminated", "anterminated"),
            ("a\x1b]funterminated", "aunterminated"),
            ("a\x1bPabc\x1b\\z", "aabc\x1b\\z"),
            ("a\x1b^abc\x07z", "a\x1b^abc\x07z"),
            ("a\x1b_abc\u{009c}z", "a\x1b_abc\u{009c}z"),
            ("a\u{0090}abc\u{009c}z", "a\u{0090}abc\u{009c}z"),
            ("a\u{009d}abc\u{009c}z", "a\u{009d}abc\u{009c}z"),
            ("a\u{009b}31mred", "ared"),
            ("a\x1b(0x", "ax"),
            ("a\x1b*0x", "a\x1b*0x"),
            ("a\x1b+c", "a\x1b+c"),
            ("a\x1b/0x", "a\x1b/0x"),
            ("a\x1bcok", "aok"),
            ("a\x1b\\ok", "a\x1b\\ok"),
        ] {
            assert_eq!(strip_ansi(input), expected, "input {input:?}");
        }
    }
}
