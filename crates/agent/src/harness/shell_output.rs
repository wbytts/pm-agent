use crate::harness::truncate::{truncate_tail, TruncationOptions, DEFAULT_MAX_BYTES};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShellCaptureResult {
    pub output: String,
    pub truncated: bool,
    pub full_output: Option<String>,
}

pub fn sanitize_binary_output(value: &str) -> String {
    value
        .chars()
        .filter(|char| {
            let code = *char as u32;
            if code == 0x09 || code == 0x0a || code == 0x0d {
                return true;
            }
            if code <= 0x1f {
                return false;
            }
            if (0xfff9..=0xfffb).contains(&code) {
                return false;
            }
            true
        })
        .collect()
}

pub fn capture_shell_output<I, S>(chunks: I, full_output_threshold: usize) -> ShellCaptureResult
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    capture_shell_output_with_options(chunks, full_output_threshold, TruncationOptions::default())
}

pub fn capture_shell_output_with_options<I, S>(
    chunks: I,
    full_output_threshold: usize,
    truncation_options: TruncationOptions,
) -> ShellCaptureResult
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    let mut rolling_chunks = Vec::<String>::new();
    let mut rolling_bytes = 0usize;
    let max_output_bytes = DEFAULT_MAX_BYTES * 2;
    let mut total_bytes = 0usize;
    let mut full_output = None::<String>;

    for chunk in chunks {
        total_bytes += chunk.as_ref().len();
        let text = sanitize_binary_output(chunk.as_ref()).replace('\r', "");
        if total_bytes > full_output_threshold {
            full_output.get_or_insert_with(|| rolling_chunks.join(""));
        }
        if let Some(full_output) = full_output.as_mut() {
            full_output.push_str(&text);
        }

        rolling_bytes += text.len();
        rolling_chunks.push(text);
        while rolling_bytes > max_output_bytes && rolling_chunks.len() > 1 {
            let removed = rolling_chunks.remove(0);
            rolling_bytes -= removed.len();
        }
    }

    let tail_output = rolling_chunks.join("");
    let truncation = truncate_tail(&tail_output, truncation_options);
    ShellCaptureResult {
        output: if truncation.truncated {
            truncation.content
        } else {
            tail_output
        },
        truncated: truncation.truncated,
        full_output,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sanitizes_binary_output_like_pi_shell_capture() {
        let text = sanitize_binary_output("ok\u{0000}\t\n\r\u{001b}x\u{fff9}y");

        assert_eq!(text, "ok\t\n\rxy");
    }

    #[test]
    fn captures_sanitized_chunks_and_tracks_large_full_output() {
        let result = capture_shell_output_with_options(
            ["line\r\n", "ok\u{0000}", "tail"].into_iter(),
            8,
            TruncationOptions {
                max_lines: 10,
                max_bytes: 6,
            },
        );

        assert_eq!(result.output, "oktail");
        assert_eq!(result.full_output.as_deref(), Some("line\noktail"));
        assert!(result.truncated);
    }
}
