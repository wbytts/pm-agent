use crate::tools::bash::run_bash;
use crate::tools::edit::{edit_file, edit_file_blocks};
use crate::tools::find::find_files;
use crate::tools::grep::grep_files;
use crate::tools::ls::list_directory;
use crate::tools::read::read_file;
use crate::tools::write::write_file;
use crate::types::{
    CodingAgentResult, CodingTool, CodingToolEdit, CodingToolKind, CodingToolRequest,
    CodingToolResult, CodingWorkspace,
};
use crate::workspace::validate_workspace;
use serde::Deserialize;
use serde_json::Value;

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

#[derive(Debug, Deserialize)]
struct RawEditRequest {
    path: String,
    #[serde(default)]
    edits: RawEditBlocks,
    #[serde(rename = "oldText")]
    old_text: Option<String>,
    #[serde(rename = "newText")]
    new_text: Option<String>,
}

#[derive(Debug, Deserialize)]
struct RawEditBlock {
    #[serde(rename = "oldText")]
    old_text: String,
    #[serde(rename = "newText")]
    new_text: String,
}

#[derive(Debug, Default)]
struct RawEditBlocks(Vec<RawEditBlock>);

impl<'de> Deserialize<'de> for RawEditBlocks {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let value = Value::deserialize(deserializer)?;
        match value {
            Value::Array(_) => serde_json::from_value(value)
                .map(Self)
                .map_err(serde::de::Error::custom),
            Value::String(raw) => serde_json::from_str::<Vec<RawEditBlock>>(&raw)
                .map(Self)
                .map_err(serde::de::Error::custom),
            _ => Err(serde::de::Error::custom(
                "edits must be an array or JSON string",
            )),
        }
    }
}

#[derive(Debug, Deserialize)]
struct RawReadRequest {
    path: String,
    offset: Option<usize>,
    limit: Option<usize>,
}

#[derive(Debug, Deserialize)]
struct RawWriteRequest {
    path: String,
    content: String,
}

#[derive(Debug, Deserialize)]
struct RawBashRequest {
    command: String,
    timeout: Option<u64>,
}

#[derive(Debug, Deserialize)]
struct RawLsRequest {
    path: Option<String>,
    limit: Option<usize>,
}

#[derive(Debug, Deserialize)]
struct RawFindRequest {
    pattern: String,
    path: Option<String>,
    limit: Option<usize>,
}

#[derive(Debug, Deserialize)]
struct RawGrepRequest {
    pattern: String,
    path: Option<String>,
    glob: Option<String>,
    #[serde(default)]
    ignore_case: bool,
    #[serde(default)]
    literal: bool,
    context: Option<usize>,
    limit: Option<usize>,
}

pub fn prepare_tool_request(name: &str, input: Value) -> Result<CodingToolRequest, String> {
    match name {
        "edit" => prepare_edit_request(input),
        "read" => {
            let input: RawReadRequest =
                serde_json::from_value(input).map_err(|error| error.to_string())?;
            Ok(CodingToolRequest::ReadFile {
                path: input.path,
                offset: input.offset,
                limit: input.limit,
            })
        }
        "write" => {
            let input: RawWriteRequest =
                serde_json::from_value(input).map_err(|error| error.to_string())?;
            Ok(CodingToolRequest::WriteFile {
                path: input.path,
                content: input.content,
            })
        }
        "bash" => {
            let input: RawBashRequest =
                serde_json::from_value(input).map_err(|error| error.to_string())?;
            Ok(CodingToolRequest::Bash {
                command: input.command,
                timeout: input.timeout,
            })
        }
        "ls" => {
            let input: RawLsRequest =
                serde_json::from_value(input).map_err(|error| error.to_string())?;
            Ok(CodingToolRequest::Ls {
                path: input.path,
                limit: input.limit,
            })
        }
        "find" => {
            let input: RawFindRequest =
                serde_json::from_value(input).map_err(|error| error.to_string())?;
            Ok(CodingToolRequest::Find {
                pattern: input.pattern,
                path: input.path,
                limit: input.limit,
            })
        }
        "grep" => {
            let input: RawGrepRequest =
                serde_json::from_value(input).map_err(|error| error.to_string())?;
            Ok(CodingToolRequest::Grep {
                pattern: input.pattern,
                path: input.path,
                glob: input.glob,
                ignore_case: input.ignore_case,
                literal: input.literal,
                context: input.context,
                limit: input.limit,
            })
        }
        _ => Err(format!("未知工具：{name}")),
    }
}

fn prepare_edit_request(input: Value) -> Result<CodingToolRequest, String> {
    let raw: RawEditRequest = serde_json::from_value(input).map_err(|error| error.to_string())?;
    let mut edits = raw
        .edits
        .0
        .into_iter()
        .map(|edit| CodingToolEdit {
            search: edit.old_text,
            replace: edit.new_text,
        })
        .collect::<Vec<_>>();

    if let (Some(search), Some(replace)) = (raw.old_text, raw.new_text) {
        edits.push(CodingToolEdit { search, replace });
    }

    Ok(CodingToolRequest::EditFileBlocks {
        path: raw.path,
        edits,
    })
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
        CodingToolRequest::Bash { command, timeout } => run_bash(workspace, command, timeout),
        CodingToolRequest::Ls { path, limit } => list_directory(workspace, path, limit),
        CodingToolRequest::Find {
            pattern,
            path,
            limit,
        } => find_files(workspace, pattern, path, limit),
        CodingToolRequest::Grep {
            pattern,
            path,
            glob,
            ignore_case,
            literal,
            context,
            limit,
        } => grep_files(
            workspace,
            pattern,
            path,
            glob,
            ignore_case,
            literal,
            context,
            limit,
        ),
    }
}
