use std::fs;

use crate::tools::common::{success, truncate_list_output_with_details};
use crate::types::{CodingAgentError, CodingAgentResult, CodingToolResult, CodingWorkspace};
use crate::workspace::resolve_workspace_path;
use serde_json::{json, Map, Value};

pub fn list_directory(
    workspace: &CodingWorkspace,
    path: Option<String>,
    limit: Option<usize>,
) -> CodingAgentResult<CodingToolResult> {
    let path = resolve_workspace_path(workspace, path.as_deref().unwrap_or("."))?;
    if !path.exists() {
        return Err(CodingAgentError::MissingFile(path.display().to_string()));
    }
    if !path.is_dir() {
        return Err(CodingAgentError::File(format!(
            "不是目录：{}",
            path.display()
        )));
    }

    let limit = limit.unwrap_or(500);
    let mut entries = fs::read_dir(&path)
        .map_err(|error| {
            CodingAgentError::File(format!("读取目录 {} 失败：{error}", path.display()))
        })?
        .filter_map(Result::ok)
        .collect::<Vec<_>>();
    entries.sort_by_key(|entry| entry.file_name().to_string_lossy().to_lowercase());

    let mut output = Vec::new();
    let mut reached_limit = false;
    for entry in entries {
        if output.len() >= limit {
            reached_limit = true;
            break;
        }
        let mut name = entry.file_name().to_string_lossy().to_string();
        if entry.path().is_dir() {
            name.push('/');
        }
        output.push(name);
    }

    if output.is_empty() {
        return success("(empty directory)");
    }
    let mut notices = Vec::new();
    if reached_limit {
        notices.push(format!(
            "{limit} entries limit reached. Use limit={} for more",
            limit * 2
        ));
    }

    let list_output = truncate_list_output_with_details(&output.join("\n"), notices);
    let mut details = Map::new();
    if reached_limit {
        details.insert("entryLimitReached".to_string(), Value::from(limit));
    }
    if list_output.truncation.truncated {
        details.insert("truncation".to_string(), json!(list_output.truncation));
    }

    Ok(CodingToolResult {
        success: true,
        output: list_output.output,
        details: (!details.is_empty()).then_some(Value::Object(details)),
        content: None,
    })
}
