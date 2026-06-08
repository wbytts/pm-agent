use std::fs;
use std::path::{Path, PathBuf};

use crate::tools::common::{relative_display, success, truncate_list_output, IgnoreMatcher};
use crate::tools::truncate::{truncate_line, GREP_MAX_LINE_LENGTH};
use crate::types::{CodingAgentError, CodingAgentResult, CodingToolResult, CodingWorkspace};
use crate::workspace::resolve_workspace_path;

pub fn grep_files(
    workspace: &CodingWorkspace,
    pattern: String,
    path: Option<String>,
    ignore_case: bool,
    literal: bool,
    limit: Option<usize>,
) -> CodingAgentResult<CodingToolResult> {
    let root = resolve_workspace_path(workspace, path.as_deref().unwrap_or("."))?;
    if !root.exists() {
        return Err(CodingAgentError::MissingFile(root.display().to_string()));
    }
    let limit = limit.unwrap_or(100);
    let mut files = Vec::new();
    if root.is_file() {
        files.push(root.clone());
    } else {
        let mut ignore = IgnoreMatcher::load(&root);
        collect_files(&root, &root, &mut files, &mut ignore)?;
    }

    let mut output = Vec::new();
    let mut reached_limit = false;
    let mut lines_truncated = false;
    for file in files {
        let Ok(content) = fs::read_to_string(&file) else {
            continue;
        };
        for (index, line) in content.lines().enumerate() {
            if output.len() >= limit {
                reached_limit = true;
                break;
            }
            if line_matches(line, &pattern, ignore_case, literal) {
                let relative = relative_display(&root, &file);
                let (line_text, was_truncated) = truncate_line(line, GREP_MAX_LINE_LENGTH);
                lines_truncated |= was_truncated;
                output.push(format!("{}:{}: {}", relative, index + 1, line_text));
            }
        }
        if reached_limit {
            break;
        }
    }

    if output.is_empty() {
        return success("No matches found");
    }

    let mut notices = Vec::new();
    if reached_limit {
        notices.push(format!(
            "{limit} matches limit reached. Use limit={} for more, or refine pattern",
            limit * 2
        ));
    }
    if lines_truncated {
        notices.push(format!(
            "Some lines truncated to {GREP_MAX_LINE_LENGTH} chars. Use read tool to see full lines"
        ));
    }
    success(truncate_list_output(&output.join("\n"), notices))
}

fn collect_files(
    root: &Path,
    current: &Path,
    files: &mut Vec<PathBuf>,
    ignore: &mut IgnoreMatcher,
) -> CodingAgentResult<()> {
    for entry in fs::read_dir(current)
        .map_err(|error| {
            CodingAgentError::File(format!("读取目录 {} 失败：{error}", current.display()))
        })?
        .filter_map(Result::ok)
    {
        let path = entry.path();
        if ignore.is_ignored(root, &path) {
            continue;
        }
        if path.is_dir() {
            ignore.load_from(&path);
            collect_files(root, &path, files, ignore)?;
        } else {
            files.push(path);
        }
    }
    Ok(())
}

fn line_matches(line: &str, pattern: &str, ignore_case: bool, literal: bool) -> bool {
    if literal {
        if ignore_case {
            return line.to_lowercase().contains(&pattern.to_lowercase());
        }
        return line.contains(pattern);
    }
    // 当前 Rust 侧不引入 regex 依赖，保持现有子串搜索语义。
    if ignore_case {
        line.to_lowercase().contains(&pattern.to_lowercase())
    } else {
        line.contains(pattern)
    }
}
