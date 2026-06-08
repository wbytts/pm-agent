use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use thiserror::Error;

use crate::conversation::{AssistantContentBlock, RichMessage};
use crate::utils::{AssistantMessageDiagnostic, DiagnosticTarget};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum MessageRole {
    System,
    User,
    Assistant,
    Tool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Message {
    pub role: MessageRole,
    pub content: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ToolDefinition {
    pub name: String,
    pub description: String,
    #[serde(default)]
    pub parameters: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct StreamToolCall {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub arguments: BTreeMap<String, serde_json::Value>,
    #[serde(default)]
    pub thought_signature: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum AssistantStopReason {
    Stop,
    Length,
    ToolUse,
    Error,
    Aborted,
}

impl AssistantStopReason {
    pub fn as_str(&self) -> &'static str {
        match self {
            AssistantStopReason::Stop => "stop",
            AssistantStopReason::Length => "length",
            AssistantStopReason::ToolUse => "tool_use",
            AssistantStopReason::Error => "error",
            AssistantStopReason::Aborted => "aborted",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct AssistantMessage {
    pub role: MessageRole,
    pub content: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub content_blocks: Vec<AssistantContentBlock>,
    pub usage: Usage,
    pub stop_reason: AssistantStopReason,
    #[serde(default)]
    pub error_message: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub diagnostics: Vec<AssistantMessageDiagnostic>,
}

impl AssistantMessage {
    pub fn from_text(content: impl Into<String>, usage: Usage) -> Self {
        let content = content.into();
        let content_blocks = if content.is_empty() {
            Vec::new()
        } else {
            vec![AssistantContentBlock::Text(
                crate::conversation::TextContent {
                    text: content.clone(),
                    text_signature: None,
                },
            )]
        };
        Self {
            role: MessageRole::Assistant,
            content,
            content_blocks,
            usage,
            stop_reason: AssistantStopReason::Stop,
            error_message: None,
            diagnostics: Vec::new(),
        }
    }

    pub fn error(message: impl Into<String>) -> Self {
        let message = message.into();
        Self {
            role: MessageRole::Assistant,
            content: String::new(),
            content_blocks: Vec::new(),
            usage: Usage::default(),
            stop_reason: AssistantStopReason::Error,
            error_message: Some(message),
            diagnostics: Vec::new(),
        }
    }
}

impl DiagnosticTarget for AssistantMessage {
    fn diagnostics_mut(&mut self) -> &mut Vec<AssistantMessageDiagnostic> {
        &mut self.diagnostics
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum AssistantMessageEvent {
    ThinkingStart {
        content_index: usize,
    },
    ThinkingDelta {
        content_index: usize,
        delta: String,
    },
    ThinkingEnd {
        content_index: usize,
        content: String,
    },
    TextDelta {
        text: String,
    },
    ToolCallStart {
        content_index: usize,
    },
    ToolCallDelta {
        content_index: usize,
        delta: String,
    },
    ToolCallEnd {
        content_index: usize,
        tool_call: StreamToolCall,
    },
    Usage {
        usage: Usage,
    },
    Done {
        message: AssistantMessage,
    },
    Error {
        error: AssistantMessage,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum ContentBlock {
    Text { text: String },
    Image { data: String, mime_type: String },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Model {
    pub id: String,
    pub provider: String,
    pub api: String,
    pub display_name: String,
    #[serde(default)]
    pub base_url: Option<String>,
    pub context_window: usize,
    #[serde(default)]
    pub max_tokens: Option<usize>,
    #[serde(default)]
    pub input: Vec<ModelInputKind>,
    #[serde(default)]
    pub headers: BTreeMap<String, String>,
    #[serde(default)]
    pub cost: ModelCost,
    #[serde(default)]
    pub reasoning: Option<ModelReasoning>,
    #[serde(default)]
    pub thinking_level_map: ThinkingLevelMap,
    #[serde(default)]
    pub compat: BTreeMap<String, serde_json::Value>,
}

impl Default for Model {
    fn default() -> Self {
        Self {
            id: String::new(),
            provider: String::new(),
            api: String::new(),
            display_name: String::new(),
            base_url: None,
            context_window: 0,
            max_tokens: None,
            input: vec![ModelInputKind::Text],
            headers: BTreeMap::new(),
            cost: ModelCost::default(),
            reasoning: None,
            thinking_level_map: ThinkingLevelMap::new(),
            compat: BTreeMap::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum ModelInputKind {
    Text,
    Image,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ModelCost {
    pub input: f64,
    pub output: f64,
    pub cache_read: f64,
    pub cache_write: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelReasoning {
    pub enabled: bool,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "camelCase")]
pub enum ModelThinkingLevel {
    Off,
    Minimal,
    Low,
    Medium,
    High,
    XHigh,
}

pub type ThinkingLevelMap = std::collections::BTreeMap<ModelThinkingLevel, Option<String>>;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ImagesModel {
    pub id: String,
    pub provider: String,
    pub api: String,
    pub display_name: String,
    pub base_url: String,
    #[serde(default)]
    pub input: Vec<ModelInputKind>,
    #[serde(default)]
    pub output: Vec<ModelInputKind>,
    #[serde(default)]
    pub headers: BTreeMap<String, String>,
    #[serde(default)]
    pub cost: ModelCost,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ImagesContext {
    pub input: Vec<ContentBlock>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ImagesStopReason {
    Stop,
    Error,
    Aborted,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AssistantImages {
    pub api: String,
    pub provider: String,
    pub model: String,
    pub output: Vec<ContentBlock>,
    pub response_id: Option<String>,
    pub usage: Option<Usage>,
    pub stop_reason: ImagesStopReason,
    pub error_message: Option<String>,
    pub timestamp_millis: u128,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Usage {
    pub input: u64,
    pub output: u64,
    pub cache_read: u64,
    pub cache_write: u64,
    pub total_tokens: u64,
    pub cost: UsageCost,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct UsageCost {
    pub input: f64,
    pub output: f64,
    pub cache_read: f64,
    pub cache_write: f64,
    pub total: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StreamRequest {
    pub model: Model,
    pub messages: Vec<Message>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub rich_messages: Vec<RichMessage>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tools: Vec<ToolDefinition>,
    #[serde(default)]
    pub metadata: BTreeMap<String, serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum StreamEvent {
    ThinkingStart {
        content_index: usize,
    },
    ThinkingDelta {
        content_index: usize,
        delta: String,
    },
    ThinkingEnd {
        content_index: usize,
        content: String,
        #[serde(default)]
        thinking_signature: Option<String>,
        #[serde(default)]
        redacted: bool,
    },
    TextDelta {
        text: String,
    },
    ToolCallStart {
        content_index: usize,
    },
    ToolCallDelta {
        content_index: usize,
        delta: String,
    },
    ToolCallEnd {
        content_index: usize,
        tool_call: StreamToolCall,
    },
    Usage {
        usage: Usage,
    },
    Finished {
        message: Message,
    },
    Error {
        message: String,
    },
}

#[derive(Debug, Error)]
pub enum AiError {
    #[error("模型配置缺少 provider")]
    MissingProvider,
    #[error("模型配置缺少 id")]
    MissingModel,
    #[error("模型配置缺少 api")]
    MissingApi,
    #[error("未注册的 API provider：{0}")]
    UnknownApi(String),
    #[error("模型 API 不匹配：{actual}，期望 {expected}")]
    MismatchedApi { actual: String, expected: String },
    #[error("缺少 API Key：{0}")]
    MissingApiKey(String),
    #[error("HTTP 请求失败：{0}")]
    Http(String),
    #[error("AI 响应格式无效：{0}")]
    InvalidResponse(String),
    #[error("清理会话资源失败：{0:?}")]
    SessionResourceCleanup(Vec<String>),
}

pub type AiResult<T> = Result<T, AiError>;

pub trait LanguageModelProvider: Send + Sync {
    fn stream(&self, request: StreamRequest) -> AiResult<Vec<StreamEvent>>;
}

pub trait ImagesProvider: Send + Sync {
    fn generate_images(
        &self,
        model: ImagesModel,
        context: ImagesContext,
    ) -> AiResult<AssistantImages>;
}

pub fn validate_model(model: &Model) -> AiResult<()> {
    if model.provider.trim().is_empty() {
        return Err(AiError::MissingProvider);
    }
    if model.id.trim().is_empty() {
        return Err(AiError::MissingModel);
    }
    if model.api.trim().is_empty() {
        return Err(AiError::MissingApi);
    }
    Ok(())
}

pub fn validate_images_model(model: &ImagesModel) -> AiResult<()> {
    if model.provider.trim().is_empty() {
        return Err(AiError::MissingProvider);
    }
    if model.id.trim().is_empty() {
        return Err(AiError::MissingModel);
    }
    if model.api.trim().is_empty() {
        return Err(AiError::MissingApi);
    }
    Ok(())
}
