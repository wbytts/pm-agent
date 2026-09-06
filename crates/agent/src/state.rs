use ai::{AssistantContentBlock, MessageRole, Model, Usage, UserContentBlock};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct AgentMessage {
    pub role: MessageRole,
    pub content: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub content_blocks: Vec<AssistantContentBlock>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub user_content_blocks: Vec<UserContentBlock>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_call_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub details: Option<serde_json::Value>,
    #[serde(default)]
    pub is_error: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub usage: Option<Usage>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stop_reason: Option<ai::AssistantStopReason>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
}

impl AgentMessage {
    pub fn new(role: MessageRole, content: impl Into<String>) -> Self {
        Self {
            role,
            content: content.into(),
            content_blocks: Vec::new(),
            user_content_blocks: Vec::new(),
            tool_call_id: None,
            tool_name: None,
            details: None,
            is_error: false,
            usage: None,
            stop_reason: None,
            provider: None,
            model: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentState {
    pub session_id: String,
    pub system_prompt: String,
    pub model: Model,
    pub messages: Vec<AgentMessage>,
    pub is_streaming: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum AgentEvent {
    AgentStart,
    MessageDelta { text: String },
    Usage { usage: Usage },
    MessageEnd { message: AgentMessage },
    AgentEnd { messages: Vec<AgentMessage> },
    Error { message: String },
}
