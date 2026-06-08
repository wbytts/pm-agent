use crate::tools::bash::run_bash;
use crate::tools::edit::{edit_file, edit_file_blocks};
use crate::tools::find::find_files;
use crate::tools::grep::grep_files;
use crate::tools::ls::list_directory;
use crate::tools::read::read_file;
use crate::tools::write::write_file;
use crate::types::{
    CodingAgentResult, CodingTool, CodingToolKind, CodingToolRequest, CodingToolResult,
    CodingWorkspace,
};
use crate::workspace::validate_workspace;

pub fn default_tools() -> Vec<CodingTool> {
    vec![
        CodingTool {
            name: "read".to_string(),
            kind: CodingToolKind::Read,
            description: "读取工作区内文件".to_string(),
        },
        CodingTool {
            name: "write".to_string(),
            kind: CodingToolKind::Write,
            description: "写入工作区内文件".to_string(),
        },
        CodingTool {
            name: "edit".to_string(),
            kind: CodingToolKind::Edit,
            description: "基于补丁修改工作区文件".to_string(),
        },
        CodingTool {
            name: "bash".to_string(),
            kind: CodingToolKind::Bash,
            description: "在工作区执行命令".to_string(),
        },
        CodingTool {
            name: "ls".to_string(),
            kind: CodingToolKind::Ls,
            description: "列出工作区内目录内容".to_string(),
        },
        CodingTool {
            name: "find".to_string(),
            kind: CodingToolKind::Find,
            description: "按 glob 模式查找工作区文件".to_string(),
        },
        CodingTool {
            name: "grep".to_string(),
            kind: CodingToolKind::Grep,
            description: "搜索工作区文件内容".to_string(),
        },
    ]
}

pub fn execute_tool(
    workspace: &CodingWorkspace,
    request: CodingToolRequest,
) -> CodingAgentResult<CodingToolResult> {
    validate_workspace(workspace)?;

    match request {
        CodingToolRequest::ReadFile {
            path,
            offset,
            limit,
        } => read_file(workspace, path, offset, limit),
        CodingToolRequest::WriteFile { path, content } => write_file(workspace, path, content),
        CodingToolRequest::EditFile {
            path,
            search,
            replace,
        } => edit_file(workspace, path, search, replace),
        CodingToolRequest::EditFileBlocks { path, edits } => {
            edit_file_blocks(workspace, path, edits)
        }
        CodingToolRequest::Bash { command } => run_bash(workspace, command),
        CodingToolRequest::Ls { path, limit } => list_directory(workspace, path, limit),
        CodingToolRequest::Find {
            pattern,
            path,
            limit,
        } => find_files(workspace, pattern, path, limit),
        CodingToolRequest::Grep {
            pattern,
            path,
            ignore_case,
            literal,
            limit,
        } => grep_files(workspace, pattern, path, ignore_case, literal, limit),
    }
}
