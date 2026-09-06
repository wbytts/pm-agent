pub const DEFAULT_MAX_LINES: usize = 2000;
pub const DEFAULT_MAX_BYTES: usize = 50 * 1024;
pub const GREP_MAX_LINE_LENGTH: usize = 500;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TruncatedBy {
    Lines,
    Bytes,
}

#[derive(Debug, Clone, PartialEq, Eq)]
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
    let lines = content.split('\n').collect::<Vec<_>>();
    let total_lines = lines.len();
    if total_lines <= options.max_lines && total_bytes <= options.max_bytes {
        return unchanged_result(content, total_lines, total_bytes, options);
    }

    let first_line_bytes = lines.first().map_or(0, |line| line.len());
    if first_line_bytes > options.max_bytes {
        return TruncationResult {
            content: String::new(),
            truncated: true,
            truncated_by: Some(TruncatedBy::Bytes),
            total_lines,
            total_bytes,
            output_lines: 0,
            output_bytes: 0,
            last_line_partial: false,
            first_line_exceeds_limit: true,
            max_lines: options.max_lines,
            max_bytes: options.max_bytes,
        };
    }

    let mut output = Vec::new();
    let mut output_bytes = 0;
    let mut truncated_by = TruncatedBy::Lines;
    for (index, line) in lines.iter().enumerate().take(options.max_lines) {
        let line_bytes = line.len() + usize::from(index > 0);
        if output_bytes + line_bytes > options.max_bytes {
            truncated_by = TruncatedBy::Bytes;
            break;
        }
        output.push(*line);
        output_bytes += line_bytes;
    }
    let content = output.join("\n");
    TruncationResult {
        output_lines: output.len(),
        output_bytes: content.len(),
        content,
        truncated: true,
        truncated_by: Some(truncated_by),
        total_lines,
        total_bytes,
        last_line_partial: false,
        first_line_exceeds_limit: false,
        max_lines: options.max_lines,
        max_bytes: options.max_bytes,
    }
}

pub fn truncate_tail(content: &str, options: TruncationOptions) -> TruncationResult {
    let total_bytes = content.len();
    let mut lines = content.split('\n').collect::<Vec<_>>();
    if lines.len() > 1 && lines.last() == Some(&"") {
        lines.pop();
    }
    let total_lines = lines.len();
    if total_lines <= options.max_lines && total_bytes <= options.max_bytes {
        return unchanged_result(content, total_lines, total_bytes, options);
    }

    let mut output = Vec::new();
    let mut output_bytes = 0;
    let mut truncated_by = TruncatedBy::Lines;
    let mut last_line_partial = false;
    for line in lines.iter().rev().take(options.max_lines) {
        let line_bytes = line.len() + usize::from(!output.is_empty());
        if output_bytes + line_bytes > options.max_bytes {
            truncated_by = TruncatedBy::Bytes;
            if output.is_empty() {
                let truncated = truncate_utf8_from_end(line, options.max_bytes);
                output.push(truncated);
                last_line_partial = true;
            }
            break;
        }
        output.push((*line).to_string());
        output_bytes += line_bytes;
    }
    output.reverse();
    let content = output.join("\n");
    TruncationResult {
        output_lines: output.len(),
        output_bytes: content.len(),
        content,
        truncated: true,
        truncated_by: Some(truncated_by),
        total_lines,
        total_bytes,
        last_line_partial,
        first_line_exceeds_limit: false,
        max_lines: options.max_lines,
        max_bytes: options.max_bytes,
    }
}

pub fn truncate_line(line: &str, max_chars: usize) -> (String, bool) {
    if line.chars().count() <= max_chars {
        return (line.to_string(), false);
    }
    (
        format!(
            "{}... [truncated]",
            line.chars().take(max_chars).collect::<String>()
        ),
        true,
    )
}

fn unchanged_result(
    content: &str,
    total_lines: usize,
    total_bytes: usize,
    options: TruncationOptions,
) -> TruncationResult {
    TruncationResult {
        content: content.to_string(),
        truncated: false,
        truncated_by: None,
        total_lines,
        total_bytes,
        output_lines: total_lines,
        output_bytes: total_bytes,
        last_line_partial: false,
        first_line_exceeds_limit: false,
        max_lines: options.max_lines,
        max_bytes: options.max_bytes,
    }
}

fn truncate_utf8_from_end(value: &str, max_bytes: usize) -> String {
    if max_bytes == 0 {
        return String::new();
    }
    let bytes = value.as_bytes();
    if bytes.len() <= max_bytes {
        return value.to_string();
    }
    let mut start = bytes.len() - max_bytes;
    while start < bytes.len() && (bytes[start] & 0b1100_0000) == 0b1000_0000 {
        start += 1;
    }
    value[start..].to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn options(max_lines: usize, max_bytes: usize) -> TruncationOptions {
        TruncationOptions {
            max_lines,
            max_bytes,
        }
    }

    #[test]
    fn format_sizes_like_pi() {
        assert_eq!(format_size(42), "42B");
        assert_eq!(format_size(1536), "1.5KB");
        assert_eq!(format_size(2 * 1024 * 1024), "2.0MB");
    }

    #[test]
    fn truncate_head_keeps_complete_utf8_lines_and_reports_first_line_limit() {
        let result = truncate_head("éé\nabc", options(10, 4));
        assert_eq!(result.content, "éé");
        assert_eq!(result.truncated_by, Some(TruncatedBy::Bytes));
        assert_eq!(result.output_bytes, 4);
        assert!(!result.first_line_exceeds_limit);

        let first_line = truncate_head("éé\nabc", options(10, 3));
        assert!(first_line.content.is_empty());
        assert!(first_line.first_line_exceeds_limit);
        assert_eq!(first_line.truncated_by, Some(TruncatedBy::Bytes));
    }

    #[test]
    fn truncate_tail_keeps_utf8_suffix_when_only_partial_last_line_fits() {
        let result = truncate_tail("aé🙂b", options(10, 5));
        assert_eq!(result.content, "🙂b");
        assert_eq!(result.output_bytes, 5);
        assert!(result.last_line_partial);
        assert_eq!(result.truncated_by, Some(TruncatedBy::Bytes));
    }

    #[test]
    fn truncate_tail_partially_keeps_oversized_single_line_with_trailing_newline() {
        let input = format!("{}\n", "X".repeat(300_000));
        let result = truncate_tail(&input, options(100, 1024));

        assert_eq!(result.content, "X".repeat(1024));
        assert_eq!(result.output_bytes, 1024);
        assert_eq!(result.output_lines, 1);
        assert!(result.last_line_partial);
        assert_eq!(result.truncated_by, Some(TruncatedBy::Bytes));
    }

    #[test]
    fn truncate_tail_drops_oversized_character_that_cannot_fit() {
        let result = truncate_tail("abc🙂", options(10, 3));
        assert_eq!(result.content, "");
        assert_eq!(result.output_bytes, 0);
        assert!(result.last_line_partial);
    }

    #[test]
    fn truncate_head_counts_trailing_newline_like_pi_harness() {
        let result = truncate_head("one\n", options(1, 100));

        assert_eq!(result.content, "one");
        assert!(result.truncated);
        assert_eq!(result.truncated_by, Some(TruncatedBy::Lines));
        assert_eq!(result.total_lines, 2);
        assert_eq!(result.output_lines, 1);
    }

    #[test]
    fn truncate_line_appends_pi_suffix() {
        assert_eq!(truncate_line("abcdef", 10), ("abcdef".to_string(), false));
        assert_eq!(
            truncate_line("abcdef", 3),
            ("abc... [truncated]".to_string(), true)
        );
    }
}
