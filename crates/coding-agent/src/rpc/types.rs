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
        #[serde(default, alias = "streamingBehavior")]
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
        #[serde(default, alias = "parentSession")]
        parent_session: Option<String>,
    },
    Reload {
        id: Option<String>,
    },
    GetState {
        id: Option<String>,
    },
    SetModel {
        id: Option<String>,
        provider: String,
        #[serde(alias = "modelId")]
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
        #[serde(default, alias = "customInstructions")]
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
    GetSessionInfo {
        id: Option<String>,
    },
    GetChangelog {
        id: Option<String>,
    },
    ExportHtml {
        id: Option<String>,
        #[serde(default, alias = "outputPath")]
        output_path: Option<String>,
    },
    ShareSession {
        id: Option<String>,
    },
    SwitchSession {
        id: Option<String>,
        #[serde(alias = "sessionPath")]
        session_path: String,
    },
    ImportSession {
        id: Option<String>,
        #[serde(alias = "inputPath")]
        input_path: String,
        #[serde(default, alias = "cwdOverride")]
        cwd_override: Option<String>,
    },
    Fork {
        id: Option<String>,
        #[serde(alias = "entryId")]
        entry_id: String,
        #[serde(default)]
        position: ForkPosition,
    },
    Clone {
        id: Option<String>,
    },
    NavigateTree {
        id: Option<String>,
        #[serde(alias = "targetId")]
        target_id: String,
        #[serde(default)]
        summarize: bool,
        #[serde(default, alias = "customInstructions")]
        custom_instructions: Option<String>,
        #[serde(default, alias = "replaceInstructions")]
        replace_instructions: bool,
        #[serde(default)]
        label: Option<String>,
    },
    GetForkMessages {
        id: Option<String>,
    },
    GetLastAssistantText {
        id: Option<String>,
    },
    CopyLastAssistantText {
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
            | Self::Reload { id }
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
            | Self::GetSessionInfo { id }
            | Self::GetChangelog { id }
            | Self::ExportHtml { id, .. }
            | Self::ShareSession { id }
            | Self::SwitchSession { id, .. }
            | Self::ImportSession { id, .. }
            | Self::Fork { id, .. }
            | Self::Clone { id }
            | Self::NavigateTree { id, .. }
            | Self::GetForkMessages { id }
            | Self::GetLastAssistantText { id }
            | Self::CopyLastAssistantText { id }
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
            Self::Reload { .. } => RpcCommandType::Reload,
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
            Self::GetSessionInfo { .. } => RpcCommandType::GetSessionInfo,
            Self::GetChangelog { .. } => RpcCommandType::GetChangelog,
            Self::ExportHtml { .. } => RpcCommandType::ExportHtml,
            Self::ShareSession { .. } => RpcCommandType::ShareSession,
            Self::SwitchSession { .. } => RpcCommandType::SwitchSession,
            Self::ImportSession { .. } => RpcCommandType::ImportSession,
            Self::Fork { .. } => RpcCommandType::Fork,
            Self::Clone { .. } => RpcCommandType::Clone,
            Self::NavigateTree { .. } => RpcCommandType::NavigateTree,
            Self::GetForkMessages { .. } => RpcCommandType::GetForkMessages,
            Self::GetLastAssistantText { .. } => RpcCommandType::GetLastAssistantText,
            Self::CopyLastAssistantText { .. } => RpcCommandType::CopyLastAssistantText,
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
            Self::Reload { .. } => Self::Reload { id: next_id },
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
            Self::GetSessionInfo { .. } => Self::GetSessionInfo { id: next_id },
            Self::GetChangelog { .. } => Self::GetChangelog { id: next_id },
            Self::ExportHtml { output_path, .. } => Self::ExportHtml {
                id: next_id,
                output_path,
            },
            Self::ShareSession { .. } => Self::ShareSession { id: next_id },
            Self::SwitchSession { session_path, .. } => Self::SwitchSession {
                id: next_id,
                session_path,
            },
            Self::ImportSession {
                input_path,
                cwd_override,
                ..
            } => Self::ImportSession {
                id: next_id,
                input_path,
                cwd_override,
            },
            Self::Fork {
                entry_id, position, ..
            } => Self::Fork {
                id: next_id,
                entry_id,
                position,
            },
            Self::Clone { .. } => Self::Clone { id: next_id },
            Self::NavigateTree {
                target_id,
                summarize,
                custom_instructions,
                replace_instructions,
                label,
                ..
            } => Self::NavigateTree {
                id: next_id,
                target_id,
                summarize,
                custom_instructions,
                replace_instructions,
                label,
            },
            Self::GetForkMessages { .. } => Self::GetForkMessages { id: next_id },
            Self::GetLastAssistantText { .. } => Self::GetLastAssistantText { id: next_id },
            Self::CopyLastAssistantText { .. } => Self::CopyLastAssistantText { id: next_id },
            Self::SetSessionName { name, .. } => Self::SetSessionName { id: next_id, name },
            Self::GetMessages { .. } => Self::GetMessages { id: next_id },
            Self::GetCommands { .. } => Self::GetCommands { id: next_id },
            prompt_or_steer => prompt_or_steer,
        }
    }
}

impl RpcCommandType {
    pub const fn all() -> &'static [Self] {
        &[
            Self::Prompt,
            Self::Steer,
            Self::FollowUp,
            Self::Abort,
            Self::NewSession,
            Self::Reload,
            Self::GetState,
            Self::SetModel,
            Self::CycleModel,
            Self::GetAvailableModels,
            Self::SetThinkingLevel,
            Self::CycleThinkingLevel,
            Self::SetSteeringMode,
            Self::SetFollowUpMode,
            Self::Compact,
            Self::SetAutoCompaction,
            Self::SetAutoRetry,
            Self::AbortRetry,
            Self::Bash,
            Self::AbortBash,
            Self::GetSessionStats,
            Self::GetSessionInfo,
            Self::GetChangelog,
            Self::ExportHtml,
            Self::ShareSession,
            Self::SwitchSession,
            Self::ImportSession,
            Self::Fork,
            Self::Clone,
            Self::NavigateTree,
            Self::GetForkMessages,
            Self::GetLastAssistantText,
            Self::CopyLastAssistantText,
            Self::SetSessionName,
            Self::GetMessages,
            Self::GetCommands,
        ]
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Prompt => "prompt",
            Self::Steer => "steer",
            Self::FollowUp => "follow_up",
            Self::Abort => "abort",
            Self::NewSession => "new_session",
            Self::Reload => "reload",
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
            Self::GetSessionInfo => "get_session_info",
            Self::GetChangelog => "get_changelog",
            Self::ExportHtml => "export_html",
            Self::ShareSession => "share_session",
            Self::SwitchSession => "switch_session",
            Self::ImportSession => "import_session",
            Self::Fork => "fork",
            Self::Clone => "clone",
            Self::NavigateTree => "navigate_tree",
            Self::GetForkMessages => "get_fork_messages",
            Self::GetLastAssistantText => "get_last_assistant_text",
            Self::CopyLastAssistantText => "copy_last_assistant_text",
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
    Reload,
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
    GetSessionInfo,
    GetChangelog,
    ExportHtml,
    ShareSession,
    SwitchSession,
    ImportSession,
    Fork,
    Clone,
    NavigateTree,
    GetForkMessages,
    GetLastAssistantText,
    CopyLastAssistantText,
    SetSessionName,
    GetMessages,
    GetCommands,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "lowercase")]
pub enum ForkPosition {
    #[default]
    Before,
    At,
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
    fn accepts_pi_rpc_camel_case_command_fields() {
        let command: RpcCommand = serde_json::from_value(serde_json::json!({
            "type": "set_model",
            "id": "model-1",
            "provider": "openai",
            "modelId": "gpt-5",
        }))
        .expect("set model command");

        match command {
            RpcCommand::SetModel { model_id, .. } => assert_eq!(model_id, "gpt-5"),
            other => panic!("expected set model command, got {other:?}"),
        }

        let command: RpcCommand = serde_json::from_value(serde_json::json!({
            "type": "new_session",
            "parentSession": "/tmp/parent.jsonl",
        }))
        .expect("new session command");

        match command {
            RpcCommand::NewSession { parent_session, .. } => {
                assert_eq!(parent_session.as_deref(), Some("/tmp/parent.jsonl"))
            }
            other => panic!("expected new session command, got {other:?}"),
        }

        let command: RpcCommand = serde_json::from_value(serde_json::json!({
            "type": "prompt",
            "message": "hi",
            "streamingBehavior": "followUp",
        }))
        .expect("prompt command");
        match command {
            RpcCommand::Prompt {
                streaming_behavior, ..
            } => assert_eq!(streaming_behavior, Some(StreamingBehavior::FollowUp)),
            other => panic!("expected prompt command, got {other:?}"),
        }

        let command: RpcCommand = serde_json::from_value(serde_json::json!({
            "type": "compact",
            "customInstructions": "short",
        }))
        .expect("compact command");
        match command {
            RpcCommand::Compact {
                custom_instructions,
                ..
            } => assert_eq!(custom_instructions.as_deref(), Some("short")),
            other => panic!("expected compact command, got {other:?}"),
        }

        let command: RpcCommand = serde_json::from_value(serde_json::json!({
            "type": "export_html",
            "outputPath": "/tmp/out.html",
        }))
        .expect("export html command");
        match command {
            RpcCommand::ExportHtml { output_path, .. } => {
                assert_eq!(output_path.as_deref(), Some("/tmp/out.html"))
            }
            other => panic!("expected export html command, got {other:?}"),
        }

        let command: RpcCommand = serde_json::from_value(serde_json::json!({
            "type": "switch_session",
            "sessionPath": "/tmp/session.jsonl",
        }))
        .expect("switch session command");
        match command {
            RpcCommand::SwitchSession { session_path, .. } => {
                assert_eq!(session_path, "/tmp/session.jsonl")
            }
            other => panic!("expected switch session command, got {other:?}"),
        }

        let command: RpcCommand = serde_json::from_value(serde_json::json!({
            "type": "import_session",
            "inputPath": "/tmp/source.jsonl",
            "cwdOverride": "/tmp/project",
        }))
        .expect("import session command");
        match command {
            RpcCommand::ImportSession {
                input_path,
                cwd_override,
                ..
            } => {
                assert_eq!(input_path, "/tmp/source.jsonl");
                assert_eq!(cwd_override.as_deref(), Some("/tmp/project"));
            }
            other => panic!("expected import session command, got {other:?}"),
        }

        let command: RpcCommand = serde_json::from_value(serde_json::json!({
            "type": "fork",
            "entryId": "entry-1",
            "position": "at",
        }))
        .expect("fork command");
        match command {
            RpcCommand::Fork {
                entry_id, position, ..
            } => {
                assert_eq!(entry_id, "entry-1");
                assert_eq!(position, ForkPosition::At);
            }
            other => panic!("expected fork command, got {other:?}"),
        }

        let command: RpcCommand = serde_json::from_value(serde_json::json!({
            "type": "navigate_tree",
            "targetId": "entry-2",
            "customInstructions": "focus",
            "replaceInstructions": true,
        }))
        .expect("navigate tree command");
        match command {
            RpcCommand::NavigateTree {
                target_id,
                custom_instructions,
                replace_instructions,
                ..
            } => {
                assert_eq!(target_id, "entry-2");
                assert_eq!(custom_instructions.as_deref(), Some("focus"));
                assert!(replace_instructions);
            }
            other => panic!("expected navigate tree command, got {other:?}"),
        }
    }

    #[test]
    fn reload_command_uses_pi_name() {
        let command: RpcCommand =
            serde_json::from_value(serde_json::json!({"type": "reload", "id": "reload-1"}))
                .expect("reload command");

        assert_eq!(command.command_type(), RpcCommandType::Reload);
        assert_eq!(command.id(), Some("reload-1"));
        assert_eq!(command.command_type().as_str(), "reload");
    }

    #[test]
    fn get_session_info_command_uses_crates_runtime_name() {
        let command: RpcCommand = serde_json::from_value(serde_json::json!({
            "type": "get_session_info",
            "id": "session-info-1",
        }))
        .expect("session info command");

        assert_eq!(command.command_type(), RpcCommandType::GetSessionInfo);
        assert_eq!(command.id(), Some("session-info-1"));
        assert_eq!(command.command_type().as_str(), "get_session_info");
    }

    #[test]
    fn get_changelog_command_uses_crates_runtime_name() {
        let command: RpcCommand = serde_json::from_value(serde_json::json!({
            "type": "get_changelog",
            "id": "changelog-1",
        }))
        .expect("changelog command");

        assert_eq!(command.command_type(), RpcCommandType::GetChangelog);
        assert_eq!(command.id(), Some("changelog-1"));
        assert_eq!(command.command_type().as_str(), "get_changelog");
    }

    #[test]
    fn navigate_tree_command_uses_pi_name() {
        let command: RpcCommand = serde_json::from_value(serde_json::json!({
            "type": "navigate_tree",
            "id": "tree-1",
            "target_id": "entry-1",
            "summarize": true,
            "custom_instructions": "focus",
            "replace_instructions": true,
            "label": "checkpoint",
        }))
        .expect("navigate tree command");

        assert_eq!(command.command_type(), RpcCommandType::NavigateTree);
        assert_eq!(command.id(), Some("tree-1"));
        assert_eq!(command.command_type().as_str(), "navigate_tree");
        match command {
            RpcCommand::NavigateTree {
                target_id,
                summarize,
                custom_instructions,
                replace_instructions,
                label,
                ..
            } => {
                assert_eq!(target_id, "entry-1");
                assert!(summarize);
                assert_eq!(custom_instructions.as_deref(), Some("focus"));
                assert!(replace_instructions);
                assert_eq!(label.as_deref(), Some("checkpoint"));
            }
            other => panic!("expected navigate tree command, got {other:?}"),
        }
    }

    #[test]
    fn import_session_command_uses_crates_runtime_name() {
        let command: RpcCommand = serde_json::from_value(serde_json::json!({
            "type": "import_session",
            "id": "import-1",
            "input_path": "/tmp/session.jsonl",
            "cwd_override": "/tmp/project",
        }))
        .expect("import session command");

        assert_eq!(command.command_type(), RpcCommandType::ImportSession);
        assert_eq!(command.id(), Some("import-1"));
        assert_eq!(command.command_type().as_str(), "import_session");
        match command {
            RpcCommand::ImportSession {
                input_path,
                cwd_override,
                ..
            } => {
                assert_eq!(input_path, "/tmp/session.jsonl");
                assert_eq!(cwd_override.as_deref(), Some("/tmp/project"));
            }
            other => panic!("expected import session command, got {other:?}"),
        }
    }

    #[test]
    fn import_session_command_defaults_cwd_override() {
        let command: RpcCommand = serde_json::from_value(serde_json::json!({
            "type": "import_session",
            "input_path": "/tmp/session.jsonl",
        }))
        .expect("import session command");

        match command {
            RpcCommand::ImportSession { cwd_override, .. } => assert_eq!(cwd_override, None),
            other => panic!("expected import session command, got {other:?}"),
        }
    }

    #[test]
    fn share_session_command_uses_crates_runtime_name() {
        let command: RpcCommand = serde_json::from_value(serde_json::json!({
            "type": "share_session",
            "id": "share-1",
        }))
        .expect("share command");

        assert_eq!(command.command_type(), RpcCommandType::ShareSession);
        assert_eq!(command.id(), Some("share-1"));
        assert_eq!(command.command_type().as_str(), "share_session");
    }

    #[test]
    fn copy_last_assistant_text_command_uses_crates_runtime_name() {
        let command: RpcCommand = serde_json::from_value(serde_json::json!({
            "type": "copy_last_assistant_text",
            "id": "copy-1",
        }))
        .expect("copy command");

        assert_eq!(
            command.command_type(),
            RpcCommandType::CopyLastAssistantText
        );
        assert_eq!(command.id(), Some("copy-1"));
        assert_eq!(command.command_type().as_str(), "copy_last_assistant_text");
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

    #[test]
    fn fork_position_defaults_to_before_like_pi() {
        let command: RpcCommand =
            serde_json::from_value(serde_json::json!({"type": "fork", "entry_id": "entry-1"}))
                .expect("fork command");

        match command {
            RpcCommand::Fork {
                entry_id, position, ..
            } => {
                assert_eq!(entry_id, "entry-1");
                assert_eq!(position, ForkPosition::Before);
            }
            other => panic!("expected fork command, got {other:?}"),
        }
    }

    #[test]
    fn fork_position_accepts_at_like_pi() {
        let command: RpcCommand = serde_json::from_value(serde_json::json!({
            "type": "fork",
            "entry_id": "entry-1",
            "position": "at",
        }))
        .expect("fork command");

        match command {
            RpcCommand::Fork { position, .. } => assert_eq!(position, ForkPosition::At),
            other => panic!("expected fork command, got {other:?}"),
        }
    }
}
