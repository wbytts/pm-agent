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
}

#[derive(Debug, Clone)]
pub struct OutputAccumulator {
    options: OutputAccumulatorOptions,
    full_output: Vec<u8>,
    decoded: String,
    temp_file_path: Option<PathBuf>,
}

impl OutputAccumulator {
    pub fn new(options: OutputAccumulatorOptions) -> Self {
        Self {
            options,
            full_output: Vec::new(),
            decoded: String::new(),
            temp_file_path: None,
        }
    }

    pub fn append(&mut self, data: &[u8]) {
        self.full_output.extend_from_slice(data);
        self.decoded.push_str(&String::from_utf8_lossy(data));
    }

    pub fn finish(&mut self) {}

    pub fn snapshot(&mut self, persist_if_truncated: bool) -> Result<OutputSnapshot, String> {
        let options = TruncationOptions {
            max_lines: self.options.max_lines,
            max_bytes: self.options.max_bytes,
        };
        let mut truncation = truncate_tail(&self.decoded, options);
        let truncated = truncation.total_lines > self.options.max_lines
            || truncation.total_bytes > self.options.max_bytes;
        if truncated {
            truncation.truncated = true;
            if truncation.truncated_by.is_none() {
                truncation.truncated_by = if truncation.total_bytes > self.options.max_bytes {
                    Some(crate::tools::truncate::TruncatedBy::Bytes)
                } else {
                    Some(crate::tools::truncate::TruncatedBy::Lines)
                };
            }
        }
        if persist_if_truncated && truncation.truncated {
            self.ensure_temp_file()?;
        }

        Ok(OutputSnapshot {
            content: truncation.content.clone(),
            truncation,
            full_output_path: self.temp_file_path.clone(),
        })
    }

    fn ensure_temp_file(&mut self) -> Result<(), String> {
        if self.temp_file_path.is_some() {
            return Ok(());
        }
        let path = temp_file_path(&self.options.temp_file_prefix);
        let mut file = fs::File::create(&path)
            .map_err(|error| format!("创建完整输出临时文件失败：{error}"))?;
        file.write_all(&self.full_output)
            .map_err(|error| format!("写入完整输出临时文件失败：{error}"))?;
        self.temp_file_path = Some(path);
        Ok(())
    }
}

fn temp_file_path(prefix: &str) -> PathBuf {
    let id = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| duration.as_nanos());
    std::env::temp_dir().join(format!("{prefix}-{id}.log"))
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
        assert!(!snapshot.truncation.truncated);
        assert!(snapshot.full_output_path.is_none());
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
        assert!(snapshot.truncation.truncated);
        let path = snapshot.full_output_path.expect("full output path");
        assert_eq!(
            fs::read_to_string(path).expect("full output should be readable"),
            "one\ntwo\nthree"
        );
    }
}
