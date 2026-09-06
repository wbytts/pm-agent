use std::fs;
use std::path::{Path, PathBuf};

use crate::tools::common::{
    glob_match, relative_display, success, truncate_list_output_with_details, IgnoreMatcher,
};
use crate::tools::truncate::{truncate_line, GREP_MAX_LINE_LENGTH};
use crate::types::{CodingAgentError, CodingAgentResult, CodingToolResult, CodingWorkspace};
use crate::workspace::resolve_workspace_path;
use serde_json::{json, Map, Value};

pub fn grep_files(
    workspace: &CodingWorkspace,
    pattern: String,
    path: Option<String>,
    glob: Option<String>,
    ignore_case: bool,
    literal: bool,
    context: Option<usize>,
    limit: Option<usize>,
) -> CodingAgentResult<CodingToolResult> {
    let root = resolve_workspace_path(workspace, path.as_deref().unwrap_or("."))?;
    if !root.exists() {
        return Err(CodingAgentError::MissingFile(root.display().to_string()));
    }
    let limit = limit.unwrap_or(100).max(1);
    let regex = if literal {
        None
    } else {
        Some(
            regex::RegexBuilder::new(&pattern)
                .case_insensitive(ignore_case)
                .build()
                .map_err(|error| CodingAgentError::File(format!("无效 grep 正则：{error}")))?,
        )
    };
    let mut files = Vec::new();
    if root.is_file() {
        if matches_glob_filter(&root, &root, glob.as_deref()) {
            files.push(root.clone());
        }
    } else {
        let mut ignore = IgnoreMatcher::load(&root);
        collect_files(&root, &root, glob.as_deref(), &mut files, &mut ignore)?;
    }

    let mut output = Vec::new();
    let mut reached_limit = false;
    let mut lines_truncated = false;
    let mut match_count = 0usize;
    let context = context.unwrap_or(0);
    for file in files {
        let Ok(content) = fs::read_to_string(&file) else {
            continue;
        };
        let lines = content
            .replace("\r\n", "\n")
            .replace('\r', "\n")
            .lines()
            .map(ToString::to_string)
            .collect::<Vec<_>>();
        for (index, line) in lines.iter().enumerate() {
            if match_count >= limit {
                reached_limit = true;
                break;
            }
            if line_matches(line, &pattern, ignore_case, literal, regex.as_ref()) {
                let relative = if root.is_file() {
                    file.file_name()
                        .and_then(|name| name.to_str())
                        .unwrap_or_default()
                        .to_string()
                } else {
                    relative_display(&root, &file)
                };
                let block = format_match_block(&relative, &lines, index, context);
                lines_truncated |= block.lines_truncated;
                output.extend(block.lines);
                match_count += 1;
                if match_count >= limit {
                    reached_limit = true;
                    break;
                }
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
    let list_output = truncate_list_output_with_details(&output.join("\n"), notices);
    let mut details = Map::new();
    if reached_limit {
        details.insert("matchLimitReached".to_string(), Value::from(limit));
    }
    if list_output.truncation.truncated {
        details.insert("truncation".to_string(), json!(list_output.truncation));
    }
    if lines_truncated {
        details.insert("linesTruncated".to_string(), Value::Bool(true));
    }

    Ok(CodingToolResult {
        success: true,
        output: list_output.output,
        details: (!details.is_empty()).then_some(Value::Object(details)),
        content: None,
    })
}

struct MatchBlock {
    lines: Vec<String>,
    lines_truncated: bool,
}

fn format_match_block(
    relative: &str,
    lines: &[String],
    match_index: usize,
    context: usize,
) -> MatchBlock {
    let start = match_index.saturating_sub(context);
    let end = (match_index + context).min(lines.len().saturating_sub(1));
    let mut output = Vec::new();
    let mut lines_truncated = false;

    for current in start..=end {
        let line = lines[current].replace('\r', "");
        let (line_text, was_truncated) = truncate_line(&line, GREP_MAX_LINE_LENGTH);
        lines_truncated |= was_truncated;
        if current == match_index {
            output.push(format!("{}:{}: {}", relative, current + 1, line_text));
        } else {
            output.push(format!("{}-{}- {}", relative, current + 1, line_text));
        }
    }

    MatchBlock {
        lines: output,
        lines_truncated,
    }
}

fn collect_files(
    root: &Path,
    current: &Path,
    glob: Option<&str>,
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
            collect_files(root, &path, glob, files, ignore)?;
        } else if matches_glob_filter(root, &path, glob) {
            files.push(path);
        }
    }
    Ok(())
}

fn matches_glob_filter(root: &Path, file: &Path, glob: Option<&str>) -> bool {
    let Some(pattern) = glob else {
        return true;
    };
    let relative = if root.is_file() {
        file.file_name()
            .and_then(|name| name.to_str())
            .unwrap_or_default()
            .to_string()
    } else {
        relative_display(root, file)
    };
    let file_name = file
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or_default();
    glob_match(&relative, pattern) || glob_match(file_name, pattern)
}

fn line_matches(
    line: &str,
    pattern: &str,
    ignore_case: bool,
    literal: bool,
    regex: Option<&regex::Regex>,
) -> bool {
    if literal {
        if ignore_case {
            return line.to_lowercase().contains(&pattern.to_lowercase());
        }
        return line.contains(pattern);
    }
    regex.is_some_and(|regex| regex.is_match(line))
}
