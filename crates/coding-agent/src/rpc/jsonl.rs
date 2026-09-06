use serde::Serialize;

pub fn serialize_json_line<T: Serialize>(value: &T) -> Result<String, serde_json::Error> {
    serde_json::to_string(value).map(|json| format!("{json}\n"))
}

#[derive(Debug, Default, Clone)]
pub struct JsonlLineReader {
    buffer: String,
    pending_utf8: Vec<u8>,
}

impl JsonlLineReader {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn push_str(&mut self, chunk: &str) -> Vec<String> {
        self.buffer.push_str(chunk);
        self.drain_complete_lines()
    }

    pub fn push_bytes(&mut self, chunk: &[u8]) -> Vec<String> {
        let mut decoded = String::new();
        append_streaming_utf8(&mut decoded, &mut self.pending_utf8, chunk);
        self.push_str(&decoded)
    }

    pub fn finish(&mut self) -> Option<String> {
        if !self.pending_utf8.is_empty() {
            self.buffer
                .push_str(&String::from_utf8_lossy(&self.pending_utf8));
            self.pending_utf8.clear();
            let lines = self.drain_complete_lines();
            if let Some(line) = lines.into_iter().next() {
                return Some(line);
            }
        }
        if self.buffer.is_empty() {
            return None;
        }
        Some(
            std::mem::take(&mut self.buffer)
                .trim_end_matches('\r')
                .to_string(),
        )
    }

    fn drain_complete_lines(&mut self) -> Vec<String> {
        let mut lines = Vec::new();
        while let Some(index) = self.buffer.find('\n') {
            let mut line = self.buffer[..index].to_string();
            if line.ends_with('\r') {
                line.pop();
            }
            lines.push(line);
            self.buffer = self.buffer[index + 1..].to_string();
        }
        lines
    }
}

fn append_streaming_utf8(output: &mut String, pending: &mut Vec<u8>, data: &[u8]) {
    if data.is_empty() {
        return;
    }

    let mut bytes = Vec::new();
    if !pending.is_empty() {
        bytes.extend_from_slice(pending);
        pending.clear();
    }
    bytes.extend_from_slice(data);

    let mut cursor = 0;
    while cursor < bytes.len() {
        match std::str::from_utf8(&bytes[cursor..]) {
            Ok(valid) => {
                output.push_str(valid);
                return;
            }
            Err(error) => {
                let valid_up_to = error.valid_up_to();
                if valid_up_to > 0 {
                    output.push_str(
                        std::str::from_utf8(&bytes[cursor..cursor + valid_up_to])
                            .expect("valid_up_to should split valid UTF-8"),
                    );
                    cursor += valid_up_to;
                }

                // JSONL 的字节流可能把多字节 UTF-8 拆在两个 chunk 中，需等下一段再解码。
                if error.error_len().is_none() {
                    pending.extend_from_slice(&bytes[cursor..]);
                    return;
                }

                output.push('\u{FFFD}');
                cursor += error.error_len().unwrap_or(1);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn serializes_one_json_record_with_lf() {
        let line = serialize_json_line(&serde_json::json!({"text": "a\nb"})).expect("json");
        assert_eq!(line, "{\"text\":\"a\\nb\"}\n");
    }

    #[test]
    fn splits_only_on_lf_and_trims_cr() {
        let mut reader = JsonlLineReader::new();
        let lines = reader.push_str("{\"a\":\"x y\"}\r\n{\"b\":1}");
        assert_eq!(lines, vec!["{\"a\":\"x y\"}"]);
        assert_eq!(reader.finish().as_deref(), Some("{\"b\":1}"));
    }

    #[test]
    fn push_bytes_decodes_utf8_split_across_chunks_like_pi_string_decoder() {
        let mut reader = JsonlLineReader::new();
        let euro = "€".as_bytes();

        assert!(reader.push_bytes(&euro[..1]).is_empty());
        let lines = reader.push_bytes(&euro[1..]);
        assert!(lines.is_empty());
        assert_eq!(reader.push_bytes(b"\n"), vec!["€"]);
    }

    #[test]
    fn emits_final_line_without_trailing_lf_and_handles_crlf_like_pi() {
        let mut reader = JsonlLineReader::new();
        let lines = reader.push_bytes(b"{\"a\":1}\r\n{\"b\":2}");

        assert_eq!(lines, vec!["{\"a\":1}"]);
        assert_eq!(reader.finish().as_deref(), Some("{\"b\":2}"));
    }

    #[test]
    fn preserves_unicode_line_separators_inside_payloads_like_pi() {
        let mut reader = JsonlLineReader::new();
        let lines = reader.push_bytes("{\"text\":\"a\u{2028}b\u{2029}c\"}\n".as_bytes());

        assert_eq!(lines, vec!["{\"text\":\"a\u{2028}b\u{2029}c\"}"]);
    }
}
