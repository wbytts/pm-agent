use crate::exec::{exec_command, ExecOptions};
use crate::tools::output_accumulator::{OutputAccumulator, OutputAccumulatorOptions};
use crate::tools::truncate::{format_size, TruncatedBy, DEFAULT_MAX_BYTES};
use crate::utils::shell::{get_shell_config, sanitize_binary_output};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default)]
pub struct BashExecutorOptions {
    pub timeout_ms: Option<u64>,
    pub shell_path: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct BashResult {
    pub output: String,
    pub exit_code: Option<i32>,
    pub cancelled: bool,
    pub truncated: bool,
    pub full_output_path: Option<String>,
}

pub fn execute_bash(
    command: &str,
    cwd: &str,
    options: Option<BashExecutorOptions>,
) -> Result<BashResult, String> {
    let options = options.unwrap_or_default();
    let shell_config = get_shell_config(options.shell_path.as_deref())?;
    let mut args = shell_config.args;
    args.push(command.to_string());
    let result = exec_command(
        &shell_config.shell,
        &args,
        cwd,
        Some(ExecOptions {
            timeout_ms: options.timeout_ms,
            cwd: Some(cwd.to_string()),
            ..ExecOptions::default()
        }),
    )?;
    let combined = sanitize_binary_output(&(result.stdout + &result.stderr)).replace('\r', "");
    let mut accumulator = OutputAccumulator::new(OutputAccumulatorOptions {
        temp_file_prefix: "pm-agent-bash".to_string(),
        ..OutputAccumulatorOptions::default()
    });
    accumulator.append(combined.as_bytes());
    accumulator.finish();
    let snapshot = accumulator.snapshot(true)?;
    let mut output = snapshot.content;
    if snapshot.truncation.truncated {
        output.push_str(&bash_truncation_note(
            &snapshot.truncation,
            snapshot.full_output_path.as_ref(),
        ));
    }
    Ok(BashResult {
        output,
        exit_code: (!result.killed).then_some(result.code),
        cancelled: result.killed,
        truncated: snapshot.truncation.truncated,
        full_output_path: snapshot
            .full_output_path
            .map(|path| path.to_string_lossy().to_string()),
    })
}

fn bash_truncation_note(
    truncation: &crate::tools::truncate::TruncationResult,
    full_output_path: Option<&std::path::PathBuf>,
) -> String {
    let full_output = full_output_path
        .map(|path| format!(" Full output: {}", path.display()))
        .unwrap_or_default();
    if truncation.last_line_partial {
        return format!(
            "\n\n[Showing last {} of line {} (line is larger than {}).{}]",
            format_size(truncation.output_bytes),
            truncation.total_lines,
            format_size(DEFAULT_MAX_BYTES),
            full_output
        );
    }
    if truncation.truncated_by == Some(TruncatedBy::Lines) {
        let start_line = truncation
            .total_lines
            .saturating_sub(truncation.output_lines)
            + 1;
        return format!(
            "\n\n[Showing lines {}-{} of {}.{}]",
            start_line, truncation.total_lines, truncation.total_lines, full_output
        );
    }
    format!(
        "\n\n[Showing {} lines ({} limit).{}]",
        truncation.output_lines,
        format_size(DEFAULT_MAX_BYTES),
        full_output
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn executes_bash_command() {
        let result = execute_bash("printf hello", ".", None).expect("bash should run");
        assert_eq!(result.output, "hello");
        assert_eq!(result.exit_code, Some(0));
    }

    #[test]
    fn truncates_bash_output_and_persists_full_output() {
        let result = execute_bash("for i in $(seq 0 2100); do echo line-$i; done", ".", None)
            .expect("bash should run");

        assert!(result.truncated);
        assert!(result.output.contains("line-2100"));
        assert!(!result.output.contains("line-0"));
        let path = result.full_output_path.expect("full output path");
        let full = std::fs::read_to_string(path).expect("full output should exist");
        assert!(full.contains("line-0"));
        assert!(full.contains("line-2100"));
    }

    #[test]
    fn executes_bash_with_custom_shell_path_like_pi() {
        let dir = std::env::temp_dir().join(format!(
            "pm-agent-bash-shell-test-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("clock")
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).expect("dir should create");
        let shell = dir.join("custom-shell");
        std::fs::write(&shell, "#!/bin/sh\nprintf custom-shell\n").expect("shell should write");
        let mut permissions = std::fs::metadata(&shell).expect("metadata").permissions();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            permissions.set_mode(0o755);
        }
        std::fs::set_permissions(&shell, permissions).expect("permissions should update");

        let result = execute_bash(
            "printf normal",
            ".",
            Some(BashExecutorOptions {
                shell_path: Some(shell.to_string_lossy().to_string()),
                ..BashExecutorOptions::default()
            }),
        )
        .expect("bash should run");

        assert_eq!(result.output, "custom-shell");
    }
}
