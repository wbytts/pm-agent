use agent::{AgentEvent, AgentMessage};
use ai::{Model, ModelThinkingLevel};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum RpcCommand {
    Prompt {
        id: Option<String>,
        message: String,
        #[serde(default)]
        images: Vec<ai::ContentBlock>,
        #[serde(default)]
        streaming_behavior: Option<StreamingBehavior>,
    },
    Steer {
        id: Option<String>,
        message: String,
        #[serde(default)]
        images: Vec<ai::ContentBlock>,
    },
    FollowUp {
        id: Option<String>,
        message: String,
        #[serde(default)]
        images: Vec<ai::ContentBlock>,
    },
    Abort {
        id: Option<String>,
    },
    NewSession {
        id: Option<String>,
        #[serde(default)]
        parent_session: Option<String>,
    },
    GetState {
        id: Option<String>,
    },
    SetModel {
        id: Option<String>,
        provider: String,
        model_id: String,
    },
    CycleModel {
        id: Option<String>,
    },
    GetAvailableModels {
        id: Option<String>,
    },
    SetThinkingLevel {
        id: Option<String>,
        level: ModelThinkingLevel,
    },
    CycleThinkingLevel {
        id: Option<String>,
    },
    SetSteeringMode {
        id: Option<String>,
        mode: QueueMode,
    },
    SetFollowUpMode {
        id: Option<String>,
        mode: QueueMode,
    },
    Compact {
        id: Option<String>,
        #[serde(default)]
        custom_instructions: Option<String>,
    },
    SetAutoCompaction {
        id: Option<String>,
        enabled: bool,
    },
    SetAutoRetry {
        id: Option<String>,
        enabled: bool,
    },
    AbortRetry {
        id: Option<String>,
    },
    Bash {
        id: Option<String>,
        command: String,
    },
    AbortBash {
        id: Option<String>,
    },
    GetSessionStats {
        id: Option<String>,
    },
    ExportHtml {
        id: Option<String>,
        #[serde(default)]
        output_path: Option<String>,
    },
    SwitchSession {
        id: Option<String>,
        session_path: String,
    },
    Fork {
        id: Option<String>,
        entry_id: String,
    },
    Clone {
        id: Option<String>,
    },
    GetForkMessages {
        id: Option<String>,
    },
    GetLastAssistantText {
        id: Option<String>,
    },
    SetSessionName {
        id: Option<String>,
        name: String,
    },
    GetMessages {
        id: Option<String>,
    },
    GetCommands {
        id: Option<String>,
    },
}

impl RpcCommand {
    pub fn prompt(message: impl Into<String>) -> Self {
        Self::Prompt {
            id: None,
            message: message.into(),
            images: Vec::new(),
            streaming_behavior: None,
        }
    }

    pub fn id(&self) -> Option<&str> {
        match self {
            Self::Prompt { id, .. }
            | Self::Steer { id, .. }
            | Self::FollowUp { id, .. }
            | Self::Abort { id }
            | Self::NewSession { id, .. }
            | Self::GetState { id }
            | Self::SetModel { id, .. }
            | Self::CycleModel { id }
            | Self::GetAvailableModels { id }
            | Self::SetThinkingLevel { id, .. }
            | Self::CycleThinkingLevel { id }
            | Self::SetSteeringMode { id, .. }
            | Self::SetFollowUpMode { id, .. }
            | Self::Compact { id, .. }
            | Self::SetAutoCompaction { id, .. }
            | Self::SetAutoRetry { id, .. }
            | Self::AbortRetry { id }
            | Self::Bash { id, .. }
            | Self::AbortBash { id }
            | Self::GetSessionStats { id }
            | Self::ExportHtml { id, .. }
            | Self::SwitchSession { id, .. }
            | Self::Fork { id, .. }
            | Self::Clone { id }
            | Self::GetForkMessages { id }
            | Self::GetLastAssistantText { id }
            | Self::SetSessionName { id, .. }
            | Self::GetMessages { id }
            | Self::GetCommands { id } => id.as_deref(),
        }
    }

    pub fn id_owned(&self) -> Option<String> {
        self.id().map(ToString::to_string)
    }

    pub fn command_type(&self) -> RpcCommandType {
        match self {
            Self::Prompt { .. } => RpcCommandType::Prompt,
            Self::Steer { .. } => RpcCommandType::Steer,
            Self::FollowUp { .. } => RpcCommandType::FollowUp,
            Self::Abort { .. } => RpcCommandType::Abort,
            Self::NewSession { .. } => RpcCommandType::NewSession,
            Self::GetState { .. } => RpcCommandType::GetState,
            Self::SetModel { .. } => RpcCommandType::SetModel,
            Self::CycleModel { .. } => RpcCommandType::CycleModel,
            Self::GetAvailableModels { .. } => RpcCommandType::GetAvailableModels,
            Self::SetThinkingLevel { .. } => RpcCommandType::SetThinkingLevel,
            Self::CycleThinkingLevel { .. } => RpcCommandType::CycleThinkingLevel,
            Self::SetSteeringMode { .. } => RpcCommandType::SetSteeringMode,
            Self::SetFollowUpMode { .. } => RpcCommandType::SetFollowUpMode,
            Self::Compact { .. } => RpcCommandType::Compact,
            Self::SetAutoCompaction { .. } => RpcCommandType::SetAutoCompaction,
            Self::SetAutoRetry { .. } => RpcCommandType::SetAutoRetry,
            Self::AbortRetry { .. } => RpcCommandType::AbortRetry,
            Self::Bash { .. } => RpcCommandType::Bash,
            Self::AbortBash { .. } => RpcCommandType::AbortBash,
            Self::GetSessionStats { .. } => RpcCommandType::GetSessionStats,
            Self::ExportHtml { .. } => RpcCommandType::ExportHtml,
            Self::SwitchSession { .. } => RpcCommandType::SwitchSession,
            Self::Fork { .. } => RpcCommandType::Fork,
            Self::Clone { .. } => RpcCommandType::Clone,
            Self::GetForkMessages { .. } => RpcCommandType::GetForkMessages,
            Self::GetLastAssistantText { .. } => RpcCommandType::GetLastAssistantText,
            Self::SetSessionName { .. } => RpcCommandType::SetSessionName,
            Self::GetMessages { .. } => RpcCommandType::GetMessages,
            Self::GetCommands { .. } => RpcCommandType::GetCommands,
        }
    }

    pub fn with_id(self, next_id: impl Into<String>) -> Self {
        let next_id = Some(next_id.into());
        match self {
            Self::Prompt {
                message,
                images,
                streaming_behavior,
                ..
            } => Self::Prompt {
                id: next_id,
                message,
                images,
                streaming_behavior,
            },
            Self::Steer {
                message, images, ..
            } => Self::Steer {
                id: next_id,
                message,
                images,
            },
            Self::FollowUp {
                message, images, ..
            } => Self::FollowUp {
                id: next_id,
                message,
                images,
            },
            other => other.replace_id(next_id),
        }
    }

    fn replace_id(self, next_id: Option<String>) -> Self {
        match self {
            Self::Abort { .. } => Self::Abort { id: next_id },
            Self::NewSession { parent_session, .. } => Self::NewSession {
                id: next_id,
                parent_session,
            },
            Self::GetState { .. } => Self::GetState { id: next_id },
            Self::SetModel {
                provider, model_id, ..
            } => Self::SetModel {
                id: next_id,
                provider,
                model_id,
            },
            Self::CycleModel { .. } => Self::CycleModel { id: next_id },
            Self::GetAvailableModels { .. } => Self::GetAvailableModels { id: next_id },
            Self::SetThinkingLevel { level, .. } => Self::SetThinkingLevel { id: next_id, level },
            Self::CycleThinkingLevel { .. } => Self::CycleThinkingLevel { id: next_id },
            Self::SetSteeringMode { mode, .. } => Self::SetSteeringMode { id: next_id, mode },
            Self::SetFollowUpMode { mode, .. } => Self::SetFollowUpMode { id: next_id, mode },
            Self::Compact {
                custom_instructions,
                ..
            } => Self::Compact {
                id: next_id,
                custom_instructions,
            },
            Self::SetAutoCompaction { enabled, .. } => Self::SetAutoCompaction {
                id: next_id,
                enabled,
            },
            Self::SetAutoRetry { enabled, .. } => Self::SetAutoRetry {
                id: next_id,
                enabled,
            },
            Self::AbortRetry { .. } => Self::AbortRetry { id: next_id },
            Self::Bash { command, .. } => Self::Bash {
                id: next_id,
                command,
            },
            Self::AbortBash { .. } => Self::AbortBash { id: next_id },
            Self::GetSessionStats { .. } => Self::GetSessionStats { id: next_id },
            Self::ExportHtml { output_path, .. } => Self::ExportHtml {
                id: next_id,
                output_path,
            },
            Self::SwitchSession { session_path, .. } => Self::SwitchSession {
                id: next_id,
                session_path,
            },
            Self::Fork { entry_id, .. } => Self::Fork {
                id: next_id,
                entry_id,
            },
            Self::Clone { .. } => Self::Clone { id: next_id },
            Self::GetForkMessages { .. } => Self::GetForkMessages { id: next_id },
            Self::GetLastAssistantText { .. } => Self::GetLastAssistantText { id: next_id },
            Self::SetSessionName { name, .. } => Self::SetSessionName { id: next_id, name },
            Self::GetMessages { .. } => Self::GetMessages { id: next_id },
            Self::GetCommands { .. } => Self::GetCommands { id: next_id },
            prompt_or_steer => prompt_or_steer,
        }
    }
}

impl RpcCommandType {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Prompt => "prompt",
            Self::Steer => "steer",
            Self::FollowUp => "follow_up",
            Self::Abort => "abort",
            Self::NewSession => "new_session",
            Self::GetState => "get_state",
            Self::SetModel => "set_model",
            Self::CycleModel => "cycle_model",
            Self::GetAvailableModels => "get_available_models",
            Self::SetThinkingLevel => "set_thinking_level",
            Self::CycleThinkingLevel => "cycle_thinking_level",
            Self::SetSteeringMode => "set_steering_mode",
            Self::SetFollowUpMode => "set_follow_up_mode",
            Self::Compact => "compact",
            Self::SetAutoCompaction => "set_auto_compaction",
            Self::SetAutoRetry => "set_auto_retry",
            Self::AbortRetry => "abort_retry",
            Self::Bash => "bash",
            Self::AbortBash => "abort_bash",
            Self::GetSessionStats => "get_session_stats",
            Self::ExportHtml => "export_html",
            Self::SwitchSession => "switch_session",
            Self::Fork => "fork",
            Self::Clone => "clone",
            Self::GetForkMessages => "get_fork_messages",
            Self::GetLastAssistantText => "get_last_assistant_text",
            Self::SetSessionName => "set_session_name",
            Self::GetMessages => "get_messages",
            Self::GetCommands => "get_commands",
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum StreamingBehavior {
    Steer,
    FollowUp,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum QueueMode {
    All,
    OneAtATime,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum RpcCommandType {
    Prompt,
    Steer,
    FollowUp,
    Abort,
    NewSession,
    GetState,
    SetModel,
    CycleModel,
    GetAvailableModels,
    SetThinkingLevel,
    CycleThinkingLevel,
    SetSteeringMode,
    SetFollowUpMode,
    Compact,
    SetAutoCompaction,
    SetAutoRetry,
    AbortRetry,
    Bash,
    AbortBash,
    GetSessionStats,
    ExportHtml,
    SwitchSession,
    Fork,
    Clone,
    GetForkMessages,
    GetLastAssistantText,
    SetSessionName,
    GetMessages,
    GetCommands,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RpcSessionState {
    pub model: Option<Model>,
    pub thinking_level: ModelThinkingLevel,
    pub is_streaming: bool,
    pub is_compacting: bool,
    pub steering_mode: QueueMode,
    pub follow_up_mode: QueueMode,
    pub session_file: Option<String>,
    pub session_id: String,
    pub session_name: Option<String>,
    pub auto_compaction_enabled: bool,
    pub message_count: usize,
    pub pending_message_count: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct RpcSlashCommand {
    pub name: String,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub argument_hint: Option<String>,
    pub source: RpcSlashCommandSource,
    pub source_info: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum RpcSlashCommandSource {
    Extension,
    Prompt,
    Skill,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum RpcResponse {
    Response {
        id: Option<String>,
        command: String,
        success: bool,
        #[serde(default)]
        data: Option<serde_json::Value>,
        #[serde(default)]
        error: Option<String>,
    },
}

impl RpcResponse {
    pub fn ok(
        id: Option<String>,
        command: impl Into<String>,
        data: Option<serde_json::Value>,
    ) -> Self {
        Self::Response {
            id,
            command: command.into(),
            success: true,
            data,
            error: None,
        }
    }

    pub fn error(id: Option<String>, command: impl Into<String>, error: impl Into<String>) -> Self {
        Self::Response {
            id,
            command: command.into(),
            success: false,
            data: None,
            error: Some(error.into()),
        }
    }

    pub fn id(&self) -> Option<&str> {
        match self {
            Self::Response { id, .. } => id.as_deref(),
        }
    }

    pub fn is_success(&self) -> bool {
        match self {
            Self::Response { success, .. } => *success,
        }
    }
}

pub type RpcEvent = AgentEvent;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum RpcExtensionUiRequest {
    ExtensionUiRequest {
        id: String,
        method: String,
        #[serde(flatten)]
        payload: serde_json::Map<String, serde_json::Value>,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum RpcExtensionUiResponse {
    ExtensionUiResponse {
        id: String,
        #[serde(flatten)]
        payload: serde_json::Map<String, serde_json::Value>,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum RpcOutput {
    Response {
        id: Option<String>,
        command: String,
        success: bool,
        #[serde(default)]
        data: Option<serde_json::Value>,
        #[serde(default)]
        error: Option<String>,
    },
    #[serde(untagged)]
    Event(AgentEvent),
    #[serde(untagged)]
    Message(AgentMessage),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn command_type_uses_pi_names() {
        let command = RpcCommand::SetFollowUpMode {
            id: Some("1".to_string()),
            mode: QueueMode::OneAtATime,
        };

        assert_eq!(command.command_type(), RpcCommandType::SetFollowUpMode);
        let value = serde_json::to_value(command).expect("json");
        assert_eq!(value["type"], "set_follow_up_mode");
        assert_eq!(value["mode"], "one-at-a-time");
    }

    #[test]
    fn response_matches_protocol_shape() {
        let response = RpcResponse::ok(
            Some("req_1".to_string()),
            "get_messages",
            Some(serde_json::json!({"messages": []})),
        );
        let value = serde_json::to_value(response).expect("json");

        assert_eq!(value["type"], "response");
        assert_eq!(value["success"], true);
        assert_eq!(value["command"], "get_messages");
    }
}
