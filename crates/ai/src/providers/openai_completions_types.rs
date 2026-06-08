use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::conversation::{RichAssistantMessage, RichMessage, ToolCall};
use crate::types::{AssistantStopReason, Usage};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum OpenAiCompletionsMaxTokensField {
    MaxTokens,
    MaxCompletionTokens,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum OpenAiCompletionsThinkingFormat {
    OpenAi,
    DeepSeek,
    OpenRouter,
    Together,
    Zai,
    Qwen,
    QwenChatTemplate,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct OpenAiCompletionsCompat {
    pub supports_store: bool,
    pub supports_developer_role: bool,
    pub supports_reasoning_effort: bool,
    pub supports_usage_in_streaming: bool,
    pub max_tokens_field: OpenAiCompletionsMaxTokensField,
    pub supports_strict_mode: bool,
    pub supports_long_cache_retention: bool,
    #[serde(default)]
    pub requires_tool_result_name: bool,
    #[serde(default)]
    pub requires_assistant_after_tool_result: bool,
    #[serde(default)]
    pub zai_tool_stream: bool,
    #[serde(default)]
    pub requires_reasoning_content_on_assistant_messages: bool,
    #[serde(default)]
    pub requires_thinking_as_text: bool,
    #[serde(default)]
    pub cache_control_format: Option<String>,
    pub thinking_format: OpenAiCompletionsThinkingFormat,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct OpenAiCompletionsOptions {
    pub max_tokens: Option<usize>,
    pub temperature: Option<f64>,
    pub session_id: Option<String>,
    pub cache_retention: Option<String>,
    pub reasoning_effort: Option<String>,
    pub tool_choice: Option<Value>,
    pub tools: Option<Vec<OpenAiCompletionsToolDefinition>>,
    pub has_tools: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct OpenAiCompletionsRequest {
    pub model: String,
    pub messages: Vec<OpenAiCompletionsMessage>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stream: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stream_options: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub store: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_tokens: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_completion_tokens: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub prompt_cache_key: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub prompt_cache_retention: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reasoning_effort: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reasoning: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub thinking: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub enable_thinking: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub chat_template_kwargs: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_choice: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_stream: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tools: Option<Vec<OpenAiCompletionsToolDefinition>>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct OpenAiCompletionsContext {
    pub system_prompt: Option<String>,
    pub messages: Vec<RichMessage>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct OpenAiCompletionsMessage {
    pub role: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content: Option<OpenAiCompletionsMessageContent>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_calls: Option<Vec<OpenAiCompletionsToolCall>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_call_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(flatten)]
    pub extra: serde_json::Map<String, Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(untagged)]
pub enum OpenAiCompletionsMessageContent {
    Text(String),
    Parts(Vec<OpenAiCompletionsContentPart>),
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum OpenAiCompletionsContentPart {
    Text {
        text: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        cache_control: Option<OpenAiCompatCacheControl>,
    },
    ImageUrl {
        image_url: OpenAiImageUrl,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct OpenAiCompatCacheControl {
    pub r#type: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ttl: Option<String>,
}

pub fn resolve_openai_completions_cache_control(
    compat: &OpenAiCompletionsCompat,
    cache_retention: Option<&str>,
) -> Option<OpenAiCompatCacheControl> {
    if compat.cache_control_format.as_deref() != Some("anthropic")
        || cache_retention == Some("none")
    {
        return None;
    }

    Some(OpenAiCompatCacheControl {
        r#type: "ephemeral".to_string(),
        ttl: (cache_retention == Some("long") && compat.supports_long_cache_retention)
            .then(|| "1h".to_string()),
    })
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct OpenAiImageUrl {
    pub url: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct OpenAiCompletionsToolCall {
    pub id: String,
    pub r#type: String,
    pub function: OpenAiCompletionsFunctionCall,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct OpenAiCompletionsToolDefinition {
    pub r#type: String,
    pub function: OpenAiCompletionsToolFunction,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cache_control: Option<OpenAiCompatCacheControl>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct OpenAiCompletionsToolFunction {
    pub name: String,
    pub description: String,
    pub parameters: Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub strict: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct OpenAiCompletionsFunctionCall {
    pub name: String,
    pub arguments: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct OpenAiCompletionsStreamChunk {
    pub id: Option<String>,
    pub model: Option<String>,
    pub choices: Vec<OpenAiCompletionsStreamChoice>,
    pub usage: Option<OpenAiCompletionsUsage>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct OpenAiCompletionsStreamChoice {
    pub delta: Option<OpenAiCompletionsStreamDelta>,
    pub finish_reason: Option<String>,
    pub usage: Option<OpenAiCompletionsUsage>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
pub struct OpenAiCompletionsStreamDelta {
    pub content: Option<String>,
    pub reasoning_content: Option<String>,
    pub reasoning: Option<String>,
    pub reasoning_text: Option<String>,
    pub tool_calls: Option<Vec<OpenAiCompletionsToolCallDelta>>,
    pub reasoning_details: Option<Vec<Value>>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct OpenAiCompletionsToolCallDelta {
    pub index: Option<usize>,
    pub id: Option<String>,
    pub function: Option<OpenAiCompletionsFunctionDelta>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct OpenAiCompletionsFunctionDelta {
    pub name: Option<String>,
    pub arguments: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
pub struct OpenAiCompletionsUsage {
    pub prompt_tokens: Option<u64>,
    pub completion_tokens: Option<u64>,
    pub prompt_cache_hit_tokens: Option<u64>,
    pub prompt_tokens_details: Option<OpenAiCompletionsPromptTokensDetails>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
pub struct OpenAiCompletionsPromptTokensDetails {
    pub cached_tokens: Option<u64>,
    pub cache_write_tokens: Option<u64>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum OpenAiCompletionsProcessedEvent {
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
pub struct OpenAiCompletionsStreamProcessResult {
    pub assistant: RichAssistantMessage,
    pub events: Vec<OpenAiCompletionsProcessedEvent>,
}
