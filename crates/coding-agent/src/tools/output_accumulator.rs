use std::fs;
use std::io::Write;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::tools::truncate::{
    truncate_tail, TruncationOptions, TruncationResult, DEFAULT_MAX_BYTES, DEFAULT_MAX_LINES,
};

#[derive(Debug, Clone)]
pub struct OutputAccumulatorOptions {
    pub max_lines: usize,
    pub max_bytes: usize,
    pub temp_file_prefix: String,
}

impl Default for OutputAccumulatorOptions {
    fn default() -> Self {
        Self {
            max_lines: DEFAULT_MAX_LINES,
            max_bytes: DEFAULT_MAX_BYTES,
            temp_file_prefix: "pm-agent-output".to_string(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OutputSnapshot {
    pub content: String,
    pub truncation: TruncationResult,
    pub full_output_path: Option<PathBuf>,
    pub last_line_bytes: usize,
}

#[derive(Debug, Clone)]
pub struct OutputAccumulator {
    options: OutputAccumulatorOptions,
    raw_chunks: Vec<u8>,
    tail_text: String,
    tail_bytes: usize,
    tail_starts_at_line_boundary: bool,
    total_raw_bytes: usize,
    total_decoded_bytes: usize,
    completed_lines: usize,
    total_lines: usize,
    current_line_bytes: usize,
    has_open_line: bool,
    pending_utf8: Vec<u8>,
    temp_file_path: Option<PathBuf>,
    temp_file_error: Option<String>,
}

impl OutputAccumulator {
    pub fn new(options: OutputAccumulatorOptions) -> Self {
        Self {
            options,
            raw_chunks: Vec::new(),
            tail_text: String::new(),
            tail_bytes: 0,
            tail_starts_at_line_boundary: true,
            total_raw_bytes: 0,
            total_decoded_bytes: 0,
            completed_lines: 0,
            total_lines: 0,
            current_line_bytes: 0,
            has_open_line: false,
            pending_utf8: Vec::new(),
            temp_file_path: None,
            temp_file_error: None,
        }
    }

    pub fn append(&mut self, data: &[u8]) {
        self.total_raw_bytes += data.len();
        if self.temp_file_path.is_some() {
            if let Err(error) = self.append_temp_file(data) {
                self.temp_file_error.get_or_insert(error);
            }
        } else {
            self.raw_chunks.extend_from_slice(data);
        }
        let mut decoded = String::new();
        append_streaming_utf8(&mut decoded, &mut self.pending_utf8, data);
        self.append_decoded_text(&decoded);
        if self.should_use_temp_file() {
            if let Err(error) = self.ensure_temp_file() {
                self.temp_file_error.get_or_insert(error);
            }
        }
    }

    pub fn finish(&mut self) {
        if !self.pending_utf8.is_empty() {
            let decoded = String::from_utf8_lossy(&self.pending_utf8).to_string();
            self.pending_utf8.clear();
            self.append_decoded_text(&decoded);
        }
        if self.should_use_temp_file() {
            if let Err(error) = self.ensure_temp_file() {
                self.temp_file_error.get_or_insert(error);
            }
        }
    }

    pub fn snapshot(&mut self, persist_if_truncated: bool) -> Result<OutputSnapshot, String> {
        let options = TruncationOptions {
            max_lines: self.options.max_lines,
            max_bytes: self.options.max_bytes,
        };
        let snapshot_text = self.snapshot_text();
        let mut truncation = truncate_tail(&snapshot_text, options);
        let truncated = self.total_lines > self.options.max_lines
            || self.total_decoded_bytes > self.options.max_bytes;
        truncation.total_lines = self.total_lines;
        truncation.total_bytes = self.total_decoded_bytes;
        if truncated {
            truncation.truncated = true;
            if truncation.truncated_by.is_none() {
                truncation.truncated_by = if self.total_decoded_bytes > self.options.max_bytes {
                    Some(crate::tools::truncate::TruncatedBy::Bytes)
                } else {
                    Some(crate::tools::truncate::TruncatedBy::Lines)
                };
            }
        }
        if persist_if_truncated && truncation.truncated {
            self.ensure_temp_file()?;
        }
        if let Some(error) = &self.temp_file_error {
            return Err(error.clone());
        }

        Ok(OutputSnapshot {
            content: truncation.content.clone(),
            truncation,
            full_output_path: self.temp_file_path.clone(),
            last_line_bytes: self.current_line_bytes,
        })
    }

    fn ensure_temp_file(&mut self) -> Result<(), String> {
        if self.temp_file_path.is_some() {
            return Ok(());
        }
        let path = temp_file_path(&self.options.temp_file_prefix);
        let mut file = fs::File::create(&path)
            .map_err(|error| format!("创建完整输出临时文件失败：{error}"))?;
        file.write_all(&self.raw_chunks)
            .map_err(|error| format!("写入完整输出临时文件失败：{error}"))?;
        self.raw_chunks.clear();
        self.temp_file_path = Some(path);
        Ok(())
    }

    fn append_temp_file(&self, data: &[u8]) -> Result<(), String> {
        let path = self
            .temp_file_path
            .as_ref()
            .ok_or_else(|| "完整输出临时文件路径缺失".to_string())?;
        let mut file = fs::OpenOptions::new()
            .append(true)
            .open(path)
            .map_err(|error| format!("打开完整输出临时文件失败：{error}"))?;
        file.write_all(data)
            .map_err(|error| format!("追加完整输出临时文件失败：{error}"))
    }

    fn should_use_temp_file(&self) -> bool {
        self.temp_file_path.is_some()
            || self.total_raw_bytes > self.options.max_bytes
            || self.raw_chunks.len() > self.options.max_bytes
            || self.total_decoded_bytes > self.options.max_bytes
            || self.total_lines > self.options.max_lines
    }

    fn append_decoded_text(&mut self, text: &str) {
        if text.is_empty() {
            return;
        }

        let bytes = text.len();
        self.total_decoded_bytes += bytes;
        self.tail_text.push_str(text);
        self.tail_bytes += bytes;
        if self.tail_bytes > self.max_rolling_bytes() * 2 {
            self.trim_tail();
        }

        let mut newlines = 0usize;
        let mut last_newline = None;
        for (index, ch) in text.char_indices() {
            if ch == '\n' {
                newlines += 1;
                last_newline = Some(index);
            }
        }

        if let Some(last_newline) = last_newline {
            self.completed_lines += newlines;
            let tail = &text[last_newline + 1..];
            self.current_line_bytes = tail.len();
            self.has_open_line = !tail.is_empty();
        } else {
            self.current_line_bytes += bytes;
            self.has_open_line = true;
        }
        self.total_lines = self.completed_lines + usize::from(self.has_open_line);
    }

    fn trim_tail(&mut self) {
        let max_rolling_bytes = self.max_rolling_bytes();
        if self.tail_text.len() <= max_rolling_bytes {
            self.tail_bytes = self.tail_text.len();
            return;
        }

        let mut start = self.tail_text.len() - max_rolling_bytes;
        while start < self.tail_text.len() && !self.tail_text.is_char_boundary(start) {
            start += 1;
        }
        let previous_byte = start
            .checked_sub(1)
            .and_then(|index| self.tail_text.as_bytes().get(index))
            .copied();
        self.tail_starts_at_line_boundary = if start == 0 {
            self.tail_starts_at_line_boundary
        } else {
            previous_byte == Some(b'\n')
        };
        self.tail_text = self.tail_text[start..].to_string();
        self.tail_bytes = self.tail_text.len();
    }

    fn snapshot_text(&self) -> String {
        if self.tail_starts_at_line_boundary {
            return self.tail_text.clone();
        }

        self.tail_text
            .find('\n')
            .map(|index| self.tail_text[index + 1..].to_string())
            .unwrap_or_else(|| self.tail_text.clone())
    }

    fn max_rolling_bytes(&self) -> usize {
        (self.options.max_bytes * 2).max(1)
    }
}

fn temp_file_path(prefix: &str) -> PathBuf {
    let id = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| duration.as_nanos());
    std::env::temp_dir().join(format!("{prefix}-{id}.log"))
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

                // chunk 末尾的不完整多字节字符要等下一次 append 再解码，保持和 pi 的 TextDecoder(stream) 一致。
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
    fn snapshots_untruncated_output_without_temp_file() {
        let mut accumulator = OutputAccumulator::new(OutputAccumulatorOptions {
            max_lines: 10,
            max_bytes: 100,
            temp_file_prefix: "pm-agent-output-test".to_string(),
        });
        accumulator.append("hello".as_bytes());

        let snapshot = accumulator
            .snapshot(true)
            .expect("snapshot should be created");

        assert_eq!(snapshot.content, "hello");
        assert_eq!(snapshot.last_line_bytes, 5);
        assert!(!snapshot.truncation.truncated);
        assert!(snapshot.full_output_path.is_none());
    }

    #[test]
    fn auto_persists_full_output_after_threshold_even_without_snapshot_request_like_pi() {
        let mut accumulator = OutputAccumulator::new(OutputAccumulatorOptions {
            max_lines: 2,
            max_bytes: 100,
            temp_file_prefix: "pm-agent-output-test".to_string(),
        });
        accumulator.append("one\ntwo\nthree".as_bytes());
        accumulator.finish();

        let snapshot = accumulator
            .snapshot(false)
            .expect("snapshot should be created");

        assert!(snapshot.truncation.truncated);
        assert!(
            accumulator.raw_chunks.is_empty(),
            "raw chunks should be released after temp file promotion"
        );
        let path = snapshot
            .full_output_path
            .expect("full output path should be created once thresholds are exceeded");
        assert_eq!(
            fs::read_to_string(path).expect("full output should be readable"),
            "one\ntwo\nthree"
        );
    }

    #[test]
    fn appends_later_chunks_to_promoted_temp_file_like_pi() {
        let mut accumulator = OutputAccumulator::new(OutputAccumulatorOptions {
            max_lines: 2,
            max_bytes: 100,
            temp_file_prefix: "pm-agent-output-test".to_string(),
        });
        accumulator.append("one\ntwo\nthree".as_bytes());
        accumulator.append("\nfour".as_bytes());
        accumulator.finish();

        let snapshot = accumulator
            .snapshot(false)
            .expect("snapshot should be created");

        assert_eq!(snapshot.content, "three\nfour");
        let path = snapshot
            .full_output_path
            .expect("full output path should be retained after promotion");
        assert_eq!(
            fs::read_to_string(path).expect("full output should be readable"),
            "one\ntwo\nthree\nfour"
        );
    }

    #[test]
    fn keeps_bounded_decoded_tail_while_reporting_total_counts_like_pi() {
        let mut accumulator = OutputAccumulator::new(OutputAccumulatorOptions {
            max_lines: 2,
            max_bytes: 12,
            temp_file_prefix: "pm-agent-output-test".to_string(),
        });
        let content = (1..=8)
            .map(|index| format!("line-{index:02}"))
            .collect::<Vec<_>>()
            .join("\n");
        accumulator.append(content.as_bytes());
        accumulator.finish();

        let snapshot = accumulator
            .snapshot(false)
            .expect("snapshot should be created");

        assert!(accumulator.tail_text.len() <= accumulator.max_rolling_bytes());
        assert_eq!(snapshot.truncation.total_lines, 8);
        assert_eq!(snapshot.truncation.total_bytes, content.len());
        assert!(snapshot.content.contains("line-08"));
        assert!(!snapshot.content.contains("line-01"));
    }

    #[test]
    fn persists_full_output_when_truncated() {
        let mut accumulator = OutputAccumulator::new(OutputAccumulatorOptions {
            max_lines: 2,
            max_bytes: 100,
            temp_file_prefix: "pm-agent-output-test".to_string(),
        });
        accumulator.append("one\ntwo\nthree".as_bytes());

        let snapshot = accumulator
            .snapshot(true)
            .expect("snapshot should be created");

        assert_eq!(snapshot.content, "two\nthree");
        assert_eq!(snapshot.last_line_bytes, 5);
        assert!(snapshot.truncation.truncated);
        let path = snapshot.full_output_path.expect("full output path");
        assert_eq!(
            fs::read_to_string(path).expect("full output should be readable"),
            "one\ntwo\nthree"
        );
    }

    #[test]
    fn decodes_utf8_split_across_chunks_like_pi_text_decoder_stream() {
        let mut accumulator = OutputAccumulator::new(OutputAccumulatorOptions {
            max_lines: 10,
            max_bytes: 100,
            temp_file_prefix: "pm-agent-output-test".to_string(),
        });
        let euro = "€\n".as_bytes();
        accumulator.append(&euro[..1]);
        accumulator.append(&euro[1..]);
        accumulator.finish();

        let snapshot = accumulator
            .snapshot(false)
            .expect("snapshot should be created");

        assert_eq!(snapshot.content, "€\n");
        assert_eq!(snapshot.truncation.total_bytes, 4);
    }

    #[test]
    fn replaces_incomplete_utf8_sequence_on_finish_like_pi_text_decoder() {
        let mut accumulator = OutputAccumulator::new(OutputAccumulatorOptions {
            max_lines: 10,
            max_bytes: 100,
            temp_file_prefix: "pm-agent-output-test".to_string(),
        });
        accumulator.append(&[0xe2, 0x82]);
        accumulator.finish();

        let snapshot = accumulator
            .snapshot(false)
            .expect("snapshot should be created");

        assert_eq!(snapshot.content, "\u{FFFD}");
    }

    #[test]
    fn reports_current_last_line_bytes_like_pi() {
        let mut accumulator = OutputAccumulator::new(OutputAccumulatorOptions {
            max_lines: 10,
            max_bytes: 4,
            temp_file_prefix: "pm-agent-output-test".to_string(),
        });
        accumulator.append("one\nééé".as_bytes());

        let snapshot = accumulator
            .snapshot(true)
            .expect("snapshot should be created");

        assert!(snapshot.truncation.last_line_partial);
        assert_eq!(snapshot.last_line_bytes, 6);
    }
}
