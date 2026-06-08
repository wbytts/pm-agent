use agent::AgentMessage;
use ai::{Model, ModelThinkingLevel};

use crate::bash_executor::BashResult;
use crate::rpc::types::{QueueMode, RpcCommand, RpcResponse, RpcSessionState, RpcSlashCommand};
use crate::session_manager::{ForkMessage, SessionStats};

pub trait RpcSessionBackend {
    fn prompt(&mut self, message: String) -> Result<(), String>;
    fn steer(&mut self, _message: String) -> Result<(), String> {
        Err("steer is not supported by this RPC backend".to_string())
    }
    fn follow_up(&mut self, _message: String) -> Result<(), String> {
        Err("follow_up is not supported by this RPC backend".to_string())
    }
    fn abort(&mut self) -> Result<(), String> {
        Err("abort is not supported by this RPC backend".to_string())
    }
    fn new_session(
        &mut self,
        _parent_session: Option<String>,
    ) -> Result<serde_json::Value, String> {
        Err("new_session is not supported by this RPC backend".to_string())
    }
    fn state(&self) -> Result<RpcSessionState, String>;
    fn set_model(&mut self, provider: String, model_id: String) -> Result<Model, String>;
    fn cycle_model(&mut self) -> Result<Option<serde_json::Value>, String> {
        Err("cycle_model is not supported by this RPC backend".to_string())
    }
    fn available_models(&self) -> Result<Vec<Model>, String>;
    fn set_thinking_level(&mut self, _level: ModelThinkingLevel) -> Result<(), String> {
        Err("set_thinking_level is not supported by this RPC backend".to_string())
    }
    fn cycle_thinking_level(&mut self) -> Result<Option<ModelThinkingLevel>, String> {
        Err("cycle_thinking_level is not supported by this RPC backend".to_string())
    }
    fn set_steering_mode(&mut self, _mode: QueueMode) -> Result<(), String> {
        Err("set_steering_mode is not supported by this RPC backend".to_string())
    }
    fn set_follow_up_mode(&mut self, _mode: QueueMode) -> Result<(), String> {
        Err("set_follow_up_mode is not supported by this RPC backend".to_string())
    }
    fn compact(
        &mut self,
        _custom_instructions: Option<String>,
    ) -> Result<serde_json::Value, String> {
        Err("compact is not supported by this RPC backend".to_string())
    }
    fn set_auto_compaction(&mut self, _enabled: bool) -> Result<(), String> {
        Err("set_auto_compaction is not supported by this RPC backend".to_string())
    }
    fn set_auto_retry(&mut self, _enabled: bool) -> Result<(), String> {
        Err("set_auto_retry is not supported by this RPC backend".to_string())
    }
    fn abort_retry(&mut self) -> Result<(), String> {
        Err("abort_retry is not supported by this RPC backend".to_string())
    }
    fn bash(&mut self, _command: String) -> Result<BashResult, String> {
        Err("bash is not supported by this RPC backend".to_string())
    }
    fn abort_bash(&mut self) -> Result<(), String> {
        Err("abort_bash is not supported by this RPC backend".to_string())
    }
    fn session_stats(&self) -> Result<SessionStats, String> {
        Err("get_session_stats is not supported by this RPC backend".to_string())
    }
    fn export_html(&mut self, _output_path: Option<String>) -> Result<String, String> {
        Err("export_html is not supported by this RPC backend".to_string())
    }
    fn switch_session(&mut self, _session_path: String) -> Result<serde_json::Value, String> {
        Err("switch_session is not supported by this RPC backend".to_string())
    }
    fn fork(&mut self, _entry_id: String) -> Result<serde_json::Value, String> {
        Err("fork is not supported by this RPC backend".to_string())
    }
    fn clone_session(&mut self) -> Result<serde_json::Value, String> {
        Err("clone is not supported by this RPC backend".to_string())
    }
    fn fork_messages(&self) -> Result<Vec<ForkMessage>, String> {
        Err("get_fork_messages is not supported by this RPC backend".to_string())
    }
    fn last_assistant_text(&self) -> Result<Option<String>, String>;
    fn set_session_name(&mut self, _name: String) -> Result<(), String> {
        Err("set_session_name is not supported by this RPC backend".to_string())
    }
    fn messages(&self) -> Result<Vec<AgentMessage>, String>;
    fn commands(&self) -> Result<Vec<RpcSlashCommand>, String> {
        Ok(Vec::new())
    }
}

pub struct RpcDispatcher<B> {
    backend: B,
}

impl<B: RpcSessionBackend> RpcDispatcher<B> {
    pub fn new(backend: B) -> Self {
        Self { backend }
    }

    pub fn backend(&self) -> &B {
        &self.backend
    }

    pub fn backend_mut(&mut self) -> &mut B {
        &mut self.backend
    }

    pub fn handle_command(&mut self, command: RpcCommand) -> RpcResponse {
        let id = command.id_owned();
        let command_type = command.command_type();
        match self.handle(command) {
            Ok(data) => RpcResponse::ok(id, command_type.as_str(), data),
            Err(error) => RpcResponse::error(id, command_type.as_str(), error),
        }
    }

    fn handle(&mut self, command: RpcCommand) -> Result<Option<serde_json::Value>, String> {
        match command {
            RpcCommand::Prompt { message, .. } => {
                self.backend.prompt(message)?;
                Ok(None)
            }
            RpcCommand::Steer { message, .. } => {
                self.backend.steer(message)?;
                Ok(None)
            }
            RpcCommand::FollowUp { message, .. } => {
                self.backend.follow_up(message)?;
                Ok(None)
            }
            RpcCommand::Abort { .. } => {
                self.backend.abort()?;
                Ok(None)
            }
            RpcCommand::NewSession { parent_session, .. } => {
                self.backend.new_session(parent_session).map(Some)
            }
            RpcCommand::GetState { .. } => to_value(self.backend.state()?),
            RpcCommand::SetModel {
                provider, model_id, ..
            } => to_value(self.backend.set_model(provider, model_id)?),
            RpcCommand::CycleModel { .. } => self.backend.cycle_model(),
            RpcCommand::GetAvailableModels { .. } => {
                to_value(serde_json::json!({ "models": self.backend.available_models()? }))
            }
            RpcCommand::SetThinkingLevel { level, .. } => {
                self.backend.set_thinking_level(level)?;
                Ok(None)
            }
            RpcCommand::CycleThinkingLevel { .. } => {
                to_value(self.backend.cycle_thinking_level()?.map(|level| {
                    serde_json::json!({
                        "level": level,
                    })
                }))
            }
            RpcCommand::SetSteeringMode { mode, .. } => {
                self.backend.set_steering_mode(mode)?;
                Ok(None)
            }
            RpcCommand::SetFollowUpMode { mode, .. } => {
                self.backend.set_follow_up_mode(mode)?;
                Ok(None)
            }
            RpcCommand::Compact {
                custom_instructions,
                ..
            } => self.backend.compact(custom_instructions).map(Some),
            RpcCommand::SetAutoCompaction { enabled, .. } => {
                self.backend.set_auto_compaction(enabled)?;
                Ok(None)
            }
            RpcCommand::SetAutoRetry { enabled, .. } => {
                self.backend.set_auto_retry(enabled)?;
                Ok(None)
            }
            RpcCommand::AbortRetry { .. } => {
                self.backend.abort_retry()?;
                Ok(None)
            }
            RpcCommand::Bash { command, .. } => to_value(self.backend.bash(command)?),
            RpcCommand::AbortBash { .. } => {
                self.backend.abort_bash()?;
                Ok(None)
            }
            RpcCommand::GetSessionStats { .. } => to_value(self.backend.session_stats()?),
            RpcCommand::ExportHtml { output_path, .. } => {
                to_value(serde_json::json!({ "path": self.backend.export_html(output_path)? }))
            }
            RpcCommand::SwitchSession { session_path, .. } => {
                self.backend.switch_session(session_path).map(Some)
            }
            RpcCommand::Fork { entry_id, .. } => self.backend.fork(entry_id).map(Some),
            RpcCommand::Clone { .. } => self.backend.clone_session().map(Some),
            RpcCommand::GetForkMessages { .. } => {
                to_value(serde_json::json!({ "messages": self.backend.fork_messages()? }))
            }
            RpcCommand::GetLastAssistantText { .. } => {
                to_value(serde_json::json!({ "text": self.backend.last_assistant_text()? }))
            }
            RpcCommand::SetSessionName { name, .. } => {
                let name = name.trim().to_string();
                if name.is_empty() {
                    return Err("Session name cannot be empty".to_string());
                }
                self.backend.set_session_name(name)?;
                Ok(None)
            }
            RpcCommand::GetMessages { .. } => {
                to_value(serde_json::json!({ "messages": self.backend.messages()? }))
            }
            RpcCommand::GetCommands { .. } => {
                to_value(serde_json::json!({ "commands": self.backend.commands()? }))
            }
        }
    }
}

fn to_value<T: serde::Serialize>(value: T) -> Result<Option<serde_json::Value>, String> {
    serde_json::to_value(value)
        .map(Some)
        .map_err(|error| error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use ai::MessageRole;

    struct TestBackend {
        messages: Vec<AgentMessage>,
        session_name: Option<String>,
        thinking_level: ModelThinkingLevel,
        auto_retry_enabled: bool,
        retry_aborted: bool,
    }

    impl Default for TestBackend {
        fn default() -> Self {
            Self {
                messages: Vec::new(),
                session_name: None,
                thinking_level: ModelThinkingLevel::Off,
                auto_retry_enabled: true,
                retry_aborted: false,
            }
        }
    }

    impl RpcSessionBackend for TestBackend {
        fn prompt(&mut self, message: String) -> Result<(), String> {
            self.messages
                .push(AgentMessage::new(MessageRole::User, message));
            Ok(())
        }

        fn state(&self) -> Result<RpcSessionState, String> {
            Ok(RpcSessionState {
                model: None,
                thinking_level: self.thinking_level,
                is_streaming: false,
                is_compacting: false,
                steering_mode: crate::rpc::types::QueueMode::OneAtATime,
                follow_up_mode: crate::rpc::types::QueueMode::OneAtATime,
                session_file: None,
                session_id: "session".to_string(),
                session_name: self.session_name.clone(),
                auto_compaction_enabled: true,
                message_count: self.messages.len(),
                pending_message_count: 0,
            })
        }

        fn set_model(&mut self, _provider: String, _model_id: String) -> Result<ai::Model, String> {
            Err("not found".to_string())
        }

        fn available_models(&self) -> Result<Vec<ai::Model>, String> {
            Ok(Vec::new())
        }

        fn set_thinking_level(&mut self, level: ModelThinkingLevel) -> Result<(), String> {
            self.thinking_level = level;
            Ok(())
        }

        fn cycle_thinking_level(&mut self) -> Result<Option<ModelThinkingLevel>, String> {
            self.thinking_level = ModelThinkingLevel::High;
            Ok(Some(self.thinking_level))
        }

        fn set_auto_retry(&mut self, enabled: bool) -> Result<(), String> {
            self.auto_retry_enabled = enabled;
            Ok(())
        }

        fn abort_retry(&mut self) -> Result<(), String> {
            self.retry_aborted = true;
            Ok(())
        }

        fn last_assistant_text(&self) -> Result<Option<String>, String> {
            Ok(self
                .messages
                .iter()
                .rev()
                .find(|message| message.role == MessageRole::Assistant)
                .map(|message| message.content.clone()))
        }

        fn set_session_name(&mut self, name: String) -> Result<(), String> {
            self.session_name = Some(name);
            Ok(())
        }

        fn messages(&self) -> Result<Vec<AgentMessage>, String> {
            Ok(self.messages.clone())
        }
    }

    #[test]
    fn dispatches_prompt_and_state() {
        let mut dispatcher = RpcDispatcher::new(TestBackend::default());
        let response = dispatcher.handle_command(RpcCommand::prompt("hello"));
        assert!(response.is_success());

        let response = dispatcher.handle_command(RpcCommand::GetState {
            id: Some("state".to_string()),
        });
        let RpcResponse::Response { data, .. } = response;
        let data = data.expect("state data");
        assert_eq!(data["messageCount"], 1);
    }

    #[test]
    fn rejects_empty_session_name_like_pi() {
        let mut dispatcher = RpcDispatcher::new(TestBackend::default());
        let response = dispatcher.handle_command(RpcCommand::SetSessionName {
            id: Some("name".to_string()),
            name: "   ".to_string(),
        });

        assert!(!response.is_success());
        let RpcResponse::Response { error, .. } = response;
        assert_eq!(error.as_deref(), Some("Session name cannot be empty"));
    }

    #[test]
    fn dispatches_thinking_level_commands_like_pi() {
        let mut dispatcher = RpcDispatcher::new(TestBackend::default());

        let response = dispatcher.handle_command(RpcCommand::SetThinkingLevel {
            id: Some("set-thinking".to_string()),
            level: ModelThinkingLevel::Medium,
        });
        assert!(response.is_success());
        assert_eq!(
            dispatcher
                .backend()
                .state()
                .expect("state should read")
                .thinking_level,
            ModelThinkingLevel::Medium
        );

        let response = dispatcher.handle_command(RpcCommand::CycleThinkingLevel {
            id: Some("cycle-thinking".to_string()),
        });
        assert!(response.is_success());
        let RpcResponse::Response { data, .. } = response;
        assert_eq!(data.expect("cycle data")["level"], "high");
        assert_eq!(
            dispatcher
                .backend()
                .state()
                .expect("state should read")
                .thinking_level,
            ModelThinkingLevel::High
        );
    }

    #[test]
    fn dispatches_retry_commands_like_pi() {
        let mut dispatcher = RpcDispatcher::new(TestBackend::default());

        let response = dispatcher.handle_command(RpcCommand::SetAutoRetry {
            id: Some("retry".to_string()),
            enabled: false,
        });
        assert!(response.is_success());
        assert!(!dispatcher.backend().auto_retry_enabled);

        let response = dispatcher.handle_command(RpcCommand::AbortRetry {
            id: Some("abort-retry".to_string()),
        });
        assert!(response.is_success());
        assert!(dispatcher.backend().retry_aborted);
    }

    #[test]
    fn returns_clear_error_for_unsupported_backend_command() {
        let mut dispatcher = RpcDispatcher::new(TestBackend::default());
        let response = dispatcher.handle_command(RpcCommand::Bash {
            id: Some("bash".to_string()),
            command: "pwd".to_string(),
        });

        assert!(!response.is_success());
        let RpcResponse::Response { error, .. } = response;
        assert_eq!(
            error.as_deref(),
            Some("bash is not supported by this RPC backend")
        );
    }
}
