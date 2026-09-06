use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::path::PathBuf;
use thiserror::Error;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CodingWorkspace {
    pub cwd: PathBuf,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum CodingToolKind {
    Read,
    Write,
    Edit,
    Bash,
    Ls,
    Find,
    Grep,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CodingTool {
    pub name: String,
    pub kind: CodingToolKind,
    pub description: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum CodingToolRequest {
    ReadFile {
        path: String,
        offset: Option<usize>,
        limit: Option<usize>,
    },
    WriteFile {
        path: String,
        content: String,
    },
    EditFile {
        path: String,
        search: String,
        replace: String,
    },
    EditFileBlocks {
        path: String,
        edits: Vec<CodingToolEdit>,
    },
    Bash {
        command: String,
        timeout: Option<u64>,
    },
    Ls {
        path: Option<String>,
        limit: Option<usize>,
    },
    Find {
        pattern: String,
        path: Option<String>,
        limit: Option<usize>,
    },
    Grep {
        pattern: String,
        path: Option<String>,
        glob: Option<String>,
        ignore_case: bool,
        literal: bool,
        context: Option<usize>,
        limit: Option<usize>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CodingToolEdit {
    pub search: String,
    pub replace: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CodingToolResult {
    pub success: bool,
    pub output: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub details: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content: Option<Vec<CodingContentBlock>>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum CodingContentBlock {
    Text { text: String },
    Image { data: String, mime_type: String },
}

#[derive(Debug, Error)]
pub enum CodingAgentError {
    #[error("工作目录不存在：{0}")]
    MissingWorkspace(String),
    #[error("工具路径必须是工作区内的相对路径：{0}")]
    UnsafePath(String),
    #[error("文件不存在：{0}")]
    MissingFile(String),
    #[error("编辑匹配内容为空")]
    EmptySearch,
    #[error("没有找到要替换的内容：{0}")]
    SearchNotFound(String),
    #[error("文件操作失败：{0}")]
    File(String),
    #[error("命令执行失败：{0}")]
    Bash(String),
}

pub type CodingAgentResult<T> = Result<T, CodingAgentError>;
