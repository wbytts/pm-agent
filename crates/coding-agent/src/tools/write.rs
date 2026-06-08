use std::fs;

use crate::tools::file_mutation_queue::with_file_mutation_queue;
use crate::types::{CodingAgentError, CodingAgentResult, CodingToolResult, CodingWorkspace};
use crate::workspace::resolve_workspace_path;

pub fn write_file(
    workspace: &CodingWorkspace,
    path: String,
    content: String,
) -> CodingAgentResult<CodingToolResult> {
    let path = resolve_workspace_path(workspace, &path)?;
    with_file_mutation_queue(&path, || {
        let byte_count = content.len();
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|error| {
                CodingAgentError::File(format!("创建目录 {} 失败：{error}", parent.display()))
            })?;
        }
        fs::write(&path, content).map_err(|error| {
            CodingAgentError::File(format!("写入文件 {} 失败：{error}", path.display()))
        })?;
        Ok(CodingToolResult {
            success: true,
            output: format!(
                "Successfully wrote {byte_count} bytes to {}",
                path.display()
            ),
            details: None,
            content: None,
        })
    })
}
