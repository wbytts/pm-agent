use crate::bash_executor::{execute_bash, BashExecutorOptions};
use crate::types::{CodingAgentError, CodingAgentResult, CodingToolResult, CodingWorkspace};
use serde_json::{json, Map, Value};

pub fn run_bash(
    workspace: &CodingWorkspace,
    command: String,
    timeout: Option<u64>,
) -> CodingAgentResult<CodingToolResult> {
    let cwd = workspace.cwd.canonicalize().map_err(|error| {
        CodingAgentError::MissingWorkspace(format!("{}：{error}", workspace.cwd.display()))
    })?;
    let options = timeout.map(|seconds| BashExecutorOptions {
        timeout_ms: Some(seconds.saturating_mul(1000)),
        ..BashExecutorOptions::default()
    });
    let result =
        execute_bash(&command, &cwd.to_string_lossy(), options).map_err(CodingAgentError::Bash)?;

    if result.cancelled {
        let seconds = timeout
            .map(|value| value.to_string())
            .unwrap_or_else(|| "unknown".to_string());
        let message = append_status(
            &result.output,
            &format!("Command timed out after {seconds} seconds"),
        );
        return Err(CodingAgentError::Bash(message));
    }

    if result.exit_code.is_some_and(|code| code != 0) {
        let code = result.exit_code.unwrap_or_default();
        let message = append_status(&result.output, &format!("Command exited with code {code}"));
        return Err(CodingAgentError::Bash(message));
    }

    let details = result.truncation.as_ref().map(|truncation| {
        let mut value = Map::new();
        value.insert("truncation".to_string(), json!(truncation));
        if let Some(path) = result.full_output_path {
            value.insert("fullOutputPath".to_string(), Value::String(path));
        }
        Value::Object(value)
    });

    Ok(CodingToolResult {
        success: true,
        output: result.output,
        details,
        content: None,
    })
}

fn append_status(output: &str, status: &str) -> String {
    if output.is_empty() {
        status.to_string()
    } else {
        format!("{output}\n\n{status}")
    }
}
