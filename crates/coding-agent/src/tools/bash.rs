use std::process::Command;

use crate::tools::common::success;
use crate::tools::truncate::{format_size, truncate_tail, TruncatedBy, TruncationOptions};
use crate::types::{CodingAgentError, CodingAgentResult, CodingToolResult, CodingWorkspace};

pub fn run_bash(
    workspace: &CodingWorkspace,
    command: String,
) -> CodingAgentResult<CodingToolResult> {
    let cwd = workspace.cwd.canonicalize().map_err(|error| {
        CodingAgentError::MissingWorkspace(format!("{}：{error}", workspace.cwd.display()))
    })?;

    let output = if cfg!(target_os = "windows") {
        Command::new("cmd")
            .args(["/C", command.as_str()])
            .current_dir(cwd)
            .output()
    } else {
        Command::new("sh")
            .args(["-lc", command.as_str()])
            .current_dir(cwd)
            .output()
    }
    .map_err(|error| CodingAgentError::Bash(error.to_string()))?;

    let mut text = String::new();
    text.push_str(&String::from_utf8_lossy(&output.stdout));
    text.push_str(&String::from_utf8_lossy(&output.stderr));
    let truncation = truncate_tail(&text, TruncationOptions::default());
    if truncation.truncated {
        text = truncation.content;
        if truncation.truncated_by == Some(TruncatedBy::Lines) {
            text.push_str(&format!(
                "\n[Truncated: showing last {} of {} lines ({} line limit)]",
                truncation.output_lines, truncation.total_lines, truncation.max_lines
            ));
        } else {
            text.push_str(&format!(
                "\n[Truncated: tail output shown ({} limit)]",
                format_size(truncation.max_bytes)
            ));
        }
    }
    if !output.status.success() {
        let code = output
            .status
            .code()
            .map_or_else(|| "unknown".to_string(), |code| code.to_string());
        let message = if text.is_empty() {
            format!("Command exited with code {code}")
        } else {
            format!("{text}\n\nCommand exited with code {code}")
        };
        return Err(CodingAgentError::Bash(message));
    }

    success(text)
}
