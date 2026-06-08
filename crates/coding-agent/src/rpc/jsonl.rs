use serde::Serialize;

pub fn serialize_json_line<T: Serialize>(value: &T) -> Result<String, serde_json::Error> {
    serde_json::to_string(value).map(|json| format!("{json}\n"))
}

#[derive(Debug, Default, Clone)]
pub struct JsonlLineReader {
    buffer: String,
}

impl JsonlLineReader {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn push_str(&mut self, chunk: &str) -> Vec<String> {
        self.buffer.push_str(chunk);
        self.drain_complete_lines()
    }

    pub fn push_bytes(&mut self, chunk: &[u8]) -> Result<Vec<String>, std::str::Utf8Error> {
        let chunk = std::str::from_utf8(chunk)?;
        Ok(self.push_str(chunk))
    }

    pub fn finish(&mut self) -> Option<String> {
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
}
