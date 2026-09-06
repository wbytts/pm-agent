use std::fs;
use std::path::Path;

use crate::tools::common::{
    glob_match, relative_display, success, truncate_list_output_with_details, IgnoreMatcher,
};
use crate::types::{CodingAgentError, CodingAgentResult, CodingToolResult, CodingWorkspace};
use crate::workspace::resolve_workspace_path;
use serde_json::{json, Map, Value};

pub fn find_files(
    workspace: &CodingWorkspace,
    pattern: String,
    path: Option<String>,
    limit: Option<usize>,
) -> CodingAgentResult<CodingToolResult> {
    let root = resolve_workspace_path(workspace, path.as_deref().unwrap_or("."))?;
    if !root.exists() {
        return Err(CodingAgentError::MissingFile(root.display().to_string()));
    }
    if !root.is_dir() {
        return Err(CodingAgentError::File(format!(
            "不是目录：{}",
            root.display()
        )));
    }

    let limit = limit.unwrap_or(1000);
    let effective_pattern = effective_find_pattern(&pattern);
    let mut results = Vec::new();
    let mut ignore = IgnoreMatcher::load(&root);
    collect_find_results(
        &root,
        &root,
        &effective_pattern,
        limit,
        &mut results,
        &mut ignore,
    )?;
    let reached_limit = results.len() >= limit;
    if results.is_empty() {
        return success("No files found matching pattern");
    }

    let mut notices = Vec::new();
    if reached_limit {
        notices.push(format!(
            "{limit} results limit reached. Use limit={} for more, or refine pattern",
            limit * 2
        ));
    }
    let list_output = truncate_list_output_with_details(&results.join("\n"), notices);
    let mut details = Map::new();
    if reached_limit {
        details.insert("resultLimitReached".to_string(), Value::from(limit));
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

fn effective_find_pattern(pattern: &str) -> String {
    if pattern.contains('/')
        && !pattern.starts_with('/')
        && !pattern.starts_with("**/")
        && pattern != "**"
    {
        format!("**/{pattern}")
    } else {
        pattern.to_string()
    }
}

fn collect_find_results(
    root: &Path,
    current: &Path,
    pattern: &str,
    limit: usize,
    results: &mut Vec<String>,
    ignore: &mut IgnoreMatcher,
) -> CodingAgentResult<()> {
    if results.len() >= limit {
        return Ok(());
    }
    let mut entries = fs::read_dir(current)
        .map_err(|error| {
            CodingAgentError::File(format!("读取目录 {} 失败：{error}", current.display()))
        })?
        .filter_map(Result::ok)
        .collect::<Vec<_>>();
    entries.sort_by_key(|entry| entry.file_name().to_string_lossy().to_lowercase());

    for entry in entries {
        let path = entry.path();
        if ignore.is_ignored(root, &path) {
            continue;
        }
        if path.is_dir() {
            ignore.load_from(&path);
            collect_find_results(root, &path, pattern, limit, results, ignore)?;
        } else {
            let relative = relative_display(root, &path);
            let file_name = path
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or_default();
            if glob_match(&relative, pattern) || glob_match(file_name, pattern) {
                results.push(relative);
                if results.len() >= limit {
                    return Ok(());
                }
            }
        }
    }
    Ok(())
}
