use std::fs;

use crate::tools::edit_diff::{
    apply_edits_to_normalized_content, detect_line_ending, generate_diff_string,
    generate_unified_patch, normalize_to_lf, restore_line_endings, strip_bom, Edit,
};
use crate::tools::file_mutation_queue::with_file_mutation_queue;
use crate::types::{
    CodingAgentError, CodingAgentResult, CodingToolEdit, CodingToolResult, CodingWorkspace,
};
use crate::workspace::resolve_workspace_path;
use serde_json::json;

pub fn edit_file(
    workspace: &CodingWorkspace,
    path: String,
    search: String,
    replace: String,
) -> CodingAgentResult<CodingToolResult> {
    edit_file_blocks(workspace, path, vec![CodingToolEdit { search, replace }])
}

pub fn edit_file_blocks(
    workspace: &CodingWorkspace,
    path: String,
    edits: Vec<CodingToolEdit>,
) -> CodingAgentResult<CodingToolResult> {
    if edits.is_empty() {
        return Err(CodingAgentError::EmptySearch);
    }

    let path = resolve_workspace_path(workspace, &path)?;
    with_file_mutation_queue(&path, || {
        if !path.exists() {
            return Err(CodingAgentError::MissingFile(path.display().to_string()));
        }

        let content = fs::read_to_string(&path).map_err(|error| {
            CodingAgentError::File(format!("读取文件 {} 失败：{error}", path.display()))
        })?;
        let line_ending = detect_line_ending(&content);
        let (bom, text) = strip_bom(&content);
        let normalized_content = normalize_to_lf(text);
        let applied = apply_edits_to_normalized_content(
            &normalized_content,
            &edits
                .iter()
                .map(|edit| Edit {
                    old_text: edit.search.clone(),
                    new_text: edit.replace.clone(),
                })
                .collect::<Vec<_>>(),
            &path.display().to_string(),
        )
        .map_err(|message| {
            if message.starts_with("Could not find") {
                CodingAgentError::SearchNotFound(path.display().to_string())
            } else if message.contains("oldText must not be empty") {
                CodingAgentError::EmptySearch
            } else {
                CodingAgentError::File(message)
            }
        })?;

        let next_content = format!(
            "{bom}{}",
            restore_line_endings(&applied.new_content, line_ending)
        );
        fs::write(&path, next_content).map_err(|error| {
            CodingAgentError::File(format!("写入文件 {} 失败：{error}", path.display()))
        })?;

        let diff = generate_diff_string(&applied.base_content, &applied.new_content, 4);
        let patch = generate_unified_patch(
            &path.display().to_string(),
            &applied.base_content,
            &applied.new_content,
        );
        Ok(CodingToolResult {
            success: true,
            output: format!(
                "Successfully replaced {} block(s) in {}.",
                edits.len(),
                path.display()
            ),
            details: Some(json!({
                "diff": diff.diff,
                "patch": patch,
                "firstChangedLine": diff.first_changed_line,
            })),
            content: None,
        })
    })
}
