use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::conversation::{RichAssistantMessage, RichMessage, ToolCall};
use crate::providers::ThinkingBudgets;
use crate::types::{AssistantStopReason, ModelThinkingLevel, Usage};

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum BedrockCacheRetention {
    None,
    Short,
    Long,
}

impl Default for BedrockCacheRetention {
    fn default() -> Self {
        Self::Short
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum BedrockThinkingDisplay {
    Summarized,
    Omitted,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct BedrockOptions {
    pub region: Option<String>,
    pub profile: Option<String>,
    pub tool_choice: Option<BedrockToolChoice>,
    pub reasoning: Option<ModelThinkingLevel>,
    pub thinking_budgets: Option<ThinkingBudgets>,
    pub interleaved_thinking: Option<bool>,
    pub thinking_display: Option<BedrockThinkingDisplay>,
    #[serde(default)]
    pub request_metadata: BTreeMap<String, String>,
    pub bearer_token: Option<String>,
    pub cache_retention: Option<BedrockCacheRetention>,
    pub max_tokens: Option<usize>,
    pub temperature: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum BedrockToolChoice {
    Auto,
    Any,
    None,
    Tool { name: String },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct BedrockToolDefinition {
    pub name: String,
    pub description: String,
    pub parameters: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct BedrockToolConfiguration {
    pub tools: Vec<BedrockToolSpec>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_choice: Option<Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct BedrockToolSpec {
    pub tool_spec: BedrockToolSpecBody,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct BedrockToolSpecBody {
    pub name: String,
    pub description: String,
    pub input_schema: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct BedrockCachePoint {
    pub r#type: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ttl: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct BedrockSystemContentBlock {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cache_point: Option<BedrockCachePoint>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct BedrockAdditionalModelRequestFields {
    #[serde(flatten)]
    pub fields: BTreeMap<String, Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct BedrockEndpointDecision {
    pub endpoint_region: Option<String>,
    pub use_explicit_endpoint: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum BedrockImageFormat {
    Jpeg,
    Png,
    Gif,
    Webp,
}

#[derive(Debug, Clone, PartialEq)]
pub struct BedrockMessagesContext {
    pub messages: Vec<RichMessage>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct BedrockMessage {
    pub role: BedrockConversationRole,
    pub content: Vec<BedrockContentBlock>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum BedrockConversationRole {
    User,
    Assistant,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct BedrockContentBlock {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub image: Option<BedrockImageBlock>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_use: Option<BedrockToolUse>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_result: Option<BedrockToolResult>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reasoning_content: Option<BedrockReasoningContent>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cache_point: Option<BedrockCachePoint>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct BedrockImageBlock {
    pub format: BedrockImageFormat,
    pub source: BedrockImageSource,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct BedrockImageSource {
    pub bytes: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct BedrockToolUse {
    pub tool_use_id: String,
    pub name: String,
    pub input: BTreeMap<String, Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct BedrockToolResult {
    pub tool_use_id: String,
    pub content: Vec<BedrockContentBlock>,
    pub status: BedrockToolResultStatus,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum BedrockToolResultStatus {
    Success,
    Error,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct BedrockReasoningContent {
    pub reasoning_text: BedrockReasoningText,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct BedrockReasoningText {
    pub text: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub signature: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum BedrockStreamEvent {
    MessageStart {
        role: BedrockConversationRole,
    },
    ContentBlockStart {
        content_block_index: usize,
        start: BedrockContentBlockStart,
    },
    ContentBlockDelta {
        content_block_index: usize,
        delta: BedrockContentBlockDelta,
    },
    ContentBlockStop {
        content_block_index: usize,
    },
    MessageStop {
        stop_reason: Option<String>,
    },
    Metadata {
        usage: Option<BedrockUsage>,
    },
    Error {
        name: Option<String>,
        message: String,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct BedrockContentBlockStart {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_use: Option<BedrockToolUseStart>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct BedrockToolUseStart {
    pub tool_use_id: Option<String>,
    pub name: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct BedrockContentBlockDelta {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_use: Option<BedrockToolUseDelta>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reasoning_content: Option<BedrockReasoningDelta>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct BedrockToolUseDelta {
    pub input: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct BedrockReasoningDelta {
    pub text: Option<String>,
    pub signature: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct BedrockUsage {
    pub input_tokens: Option<u64>,
    pub output_tokens: Option<u64>,
    pub cache_read_input_tokens: Option<u64>,
    pub cache_write_input_tokens: Option<u64>,
    pub total_tokens: Option<u64>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum BedrockProcessedEvent {
    Start,
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
    TextStart {
        content_index: usize,
    },
    TextDelta {
        content_index: usize,
        delta: String,
    },
    TextEnd {
        content_index: usize,
        content: String,
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
        tool_call: ToolCall,
    },
    Usage {
        usage: Usage,
    },
    Completed {
        stop_reason: AssistantStopReason,
    },
    Error {
        message: String,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub struct BedrockStreamProcessResult {
    pub assistant: RichAssistantMessage,
    pub events: Vec<BedrockProcessedEvent>,
}
