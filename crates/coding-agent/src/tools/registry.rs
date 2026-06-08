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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NoToolsMode {
    All,
    Builtin,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolActivationPlan {
    pub initial_active_tool_names: Vec<String>,
    pub allowed_tool_names: Option<Vec<String>>,
}

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

pub fn plan_tool_activation(
    explicit_tools: Option<Vec<String>>,
    no_tools: Option<NoToolsMode>,
) -> ToolActivationPlan {
    if let Some(explicit_tools) = explicit_tools {
        return ToolActivationPlan {
            initial_active_tool_names: explicit_tools.clone(),
            allowed_tool_names: Some(explicit_tools),
        };
    }

    let initial_active_tool_names = if no_tools.is_some() {
        Vec::new()
    } else {
        ["read", "bash", "edit", "write"]
            .into_iter()
            .map(str::to_string)
            .collect()
    };

    ToolActivationPlan {
        initial_active_tool_names,
        allowed_tool_names: matches!(no_tools, Some(NoToolsMode::All)).then(Vec::new),
    }
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
