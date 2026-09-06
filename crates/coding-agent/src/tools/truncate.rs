use serde::{Deserialize, Serialize};

pub const DEFAULT_MAX_LINES: usize = 2000;
pub const DEFAULT_MAX_BYTES: usize = 50 * 1024;
pub const GREP_MAX_LINE_LENGTH: usize = 500;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TruncatedBy {
    Lines,
    Bytes,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct TruncationResult {
    pub content: String,
    pub truncated: bool,
    pub truncated_by: Option<TruncatedBy>,
    pub total_lines: usize,
    pub total_bytes: usize,
    pub output_lines: usize,
    pub output_bytes: usize,
    pub last_line_partial: bool,
    pub first_line_exceeds_limit: bool,
    pub max_lines: usize,
    pub max_bytes: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TruncationOptions {
    pub max_lines: usize,
    pub max_bytes: usize,
}

impl Default for TruncationOptions {
    fn default() -> Self {
        Self {
            max_lines: DEFAULT_MAX_LINES,
            max_bytes: DEFAULT_MAX_BYTES,
        }
    }
}

pub fn format_size(bytes: usize) -> String {
    if bytes < 1024 {
        format!("{bytes}B")
    } else if bytes < 1024 * 1024 {
        format!("{:.1}KB", bytes as f64 / 1024.0)
    } else {
        format!("{:.1}MB", bytes as f64 / (1024.0 * 1024.0))
    }
}

pub fn truncate_head(content: &str, options: TruncationOptions) -> TruncationResult {
    let total_bytes = content.len();
    let lines = split_lines_for_counting(content);
    let total_lines = lines.len();

    if total_lines <= options.max_lines && total_bytes <= options.max_bytes {
        return result(
            content.to_string(),
            false,
            None,
            total_lines,
            total_bytes,
            total_lines,
            total_bytes,
            false,
            false,
            options,
        );
    }

    if lines
        .first()
        .is_some_and(|line| line.len() > options.max_bytes)
    {
        return result(
            String::new(),
            true,
            Some(TruncatedBy::Bytes),
            total_lines,
            total_bytes,
            0,
            0,
            false,
            true,
            options,
        );
    }

    let mut output_lines = Vec::new();
    let mut output_bytes = 0;
    let mut truncated_by = TruncatedBy::Lines;
    for (index, line) in lines.iter().enumerate().take(options.max_lines) {
        let line_bytes = line.len() + usize::from(index > 0);
        if output_bytes + line_bytes > options.max_bytes {
            truncated_by = TruncatedBy::Bytes;
            break;
        }
        output_lines.push(*line);
        output_bytes += line_bytes;
    }

    let content = output_lines.join("\n");
    let output_bytes = content.len();
    result(
        content,
        true,
        Some(truncated_by),
        total_lines,
        total_bytes,
        output_lines.len(),
        output_bytes,
        false,
        false,
        options,
    )
}

pub fn truncate_tail(content: &str, options: TruncationOptions) -> TruncationResult {
    let total_bytes = content.len();
    let lines = split_lines_for_counting(content);
    let total_lines = lines.len();

    if total_lines <= options.max_lines && total_bytes <= options.max_bytes {
        return result(
            content.to_string(),
            false,
            None,
            total_lines,
            total_bytes,
            total_lines,
            total_bytes,
            false,
            false,
            options,
        );
    }

    let mut output_lines = Vec::new();
    let mut output_bytes = 0;
    let mut truncated_by = TruncatedBy::Lines;
    let mut last_line_partial = false;

    for line in lines.iter().rev().take(options.max_lines) {
        let line_bytes = line.len() + usize::from(!output_lines.is_empty());
        if output_bytes + line_bytes > options.max_bytes {
            truncated_by = TruncatedBy::Bytes;
            if output_lines.is_empty() {
                let truncated_line = truncate_string_to_bytes_from_end(line, options.max_bytes);
                output_lines.insert(0, truncated_line);
                last_line_partial = true;
            }
            break;
        }
        output_lines.insert(0, (*line).to_string());
        output_bytes += line_bytes;
    }

    let content = output_lines.join("\n");
    let output_bytes = content.len();
    let output_lines_len = output_lines.len();
    result(
        content,
        true,
        Some(truncated_by),
        total_lines,
        total_bytes,
        output_lines_len,
        output_bytes,
        last_line_partial,
        false,
        options,
    )
}

pub fn truncate_line(line: &str, max_chars: usize) -> (String, bool) {
    if line.chars().count() <= max_chars {
        return (line.to_string(), false);
    }
    let mut text = line.chars().take(max_chars).collect::<String>();
    text.push_str("... [truncated]");
    (text, true)
}

fn split_lines_for_counting(content: &str) -> Vec<&str> {
    if content.is_empty() {
        return Vec::new();
    }
    let mut lines = content.split('\n').collect::<Vec<_>>();
    if content.ends_with('\n') {
        lines.pop();
    }
    lines
}

fn truncate_string_to_bytes_from_end(value: &str, max_bytes: usize) -> String {
    if value.len() <= max_bytes {
        return value.to_string();
    }

    let mut start = value.len() - max_bytes;
    while start < value.len() && !value.is_char_boundary(start) {
        start += 1;
    }
    value[start..].to_string()
}

#[allow(clippy::too_many_arguments)]
fn result(
    content: String,
    truncated: bool,
    truncated_by: Option<TruncatedBy>,
    total_lines: usize,
    total_bytes: usize,
    output_lines: usize,
    output_bytes: usize,
    last_line_partial: bool,
    first_line_exceeds_limit: bool,
    options: TruncationOptions,
) -> TruncationResult {
    TruncationResult {
        content,
        truncated,
        truncated_by,
        total_lines,
        total_bytes,
        output_lines,
        output_bytes,
        last_line_partial,
        first_line_exceeds_limit,
        max_lines: options.max_lines,
        max_bytes: options.max_bytes,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn truncates_head_without_partial_lines() {
        let result = truncate_head(
            "one\ntwo\nthree",
            TruncationOptions {
                max_lines: 2,
                max_bytes: 100,
            },
        );

        assert_eq!(result.content, "one\ntwo");
        assert_eq!(result.truncated_by, Some(TruncatedBy::Lines));
        assert_eq!(result.total_lines, 3);
    }

    #[test]
    fn reports_first_line_exceeding_byte_limit() {
        let result = truncate_head(
            "abcdef\nsecond",
            TruncationOptions {
                max_lines: 10,
                max_bytes: 3,
            },
        );

        assert!(result.first_line_exceeds_limit);
        assert!(result.content.is_empty());
    }

    #[test]
    fn truncates_tail_from_end_on_long_line_boundary() {
        let result = truncate_tail(
            "hello🙂world",
            TruncationOptions {
                max_lines: 10,
                max_bytes: 8,
            },
        );

        assert!(result.last_line_partial);
        assert_eq!(result.content, "world");
    }

    #[test]
    fn truncates_tail_oversized_single_line_with_trailing_newline_like_pi() {
        let input = format!("{}\n", "X".repeat(300_000));
        let result = truncate_tail(
            &input,
            TruncationOptions {
                max_lines: 100,
                max_bytes: 1024,
            },
        );

        assert_eq!(result.content, "X".repeat(1024));
        assert_eq!(result.output_bytes, 1024);
        assert_eq!(result.output_lines, 1);
        assert!(result.last_line_partial);
        assert_eq!(result.truncated_by, Some(TruncatedBy::Bytes));
    }

    #[test]
    fn ignores_trailing_newline_for_tool_line_count_like_pi() {
        let result = truncate_head(
            "one\n",
            TruncationOptions {
                max_lines: 1,
                max_bytes: 100,
            },
        );

        assert_eq!(result.content, "one\n");
        assert!(!result.truncated);
        assert_eq!(result.total_lines, 1);
        assert_eq!(result.output_lines, 1);
    }

    #[test]
    fn formats_sizes_like_pi() {
        assert_eq!(format_size(12), "12B");
        assert_eq!(format_size(1536), "1.5KB");
        assert_eq!(format_size(2 * 1024 * 1024), "2.0MB");
    }
}
