use serde_json::Value;
use thiserror::Error;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Skill {
    pub name: String,
    pub description: String,
    pub content: String,
    pub file_path: String,
    pub source_info: Option<Value>,
    pub disable_model_invocation: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PromptTemplate {
    pub name: String,
    pub description: Option<String>,
    pub argument_hint: Option<String>,
    pub content: String,
    pub file_path: String,
    pub source_info: Option<Value>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SessionErrorCode {
    NotFound,
    InvalidSession,
    InvalidEntry,
    InvalidForkTarget,
    Storage,
    Unknown,
}

#[derive(Debug, Error)]
#[error("{code:?}: {message}")]
pub struct SessionError {
    pub code: SessionErrorCode,
    pub message: String,
}

impl SessionError {
    pub fn new(code: SessionErrorCode, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
        }
    }
}

pub type SessionResult<T> = Result<T, SessionError>;
