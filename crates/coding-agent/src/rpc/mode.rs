use crate::rpc::dispatcher::{RpcDispatcher, RpcSessionBackend};
use crate::rpc::jsonl::{serialize_json_line, JsonlLineReader};
use crate::rpc::types::{RpcCommand, RpcExtensionUiResponse, RpcResponse};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RpcModeOutput {
    pub line: String,
}

pub struct RpcMode<B: RpcSessionBackend> {
    dispatcher: RpcDispatcher<B>,
    reader: JsonlLineReader,
}

impl<B: RpcSessionBackend> RpcMode<B> {
    pub fn new(backend: B) -> Self {
        Self {
            dispatcher: RpcDispatcher::new(backend),
            reader: JsonlLineReader::new(),
        }
    }

    pub fn dispatcher(&self) -> &RpcDispatcher<B> {
        &self.dispatcher
    }

    pub fn dispatcher_mut(&mut self) -> &mut RpcDispatcher<B> {
        &mut self.dispatcher
    }

    pub fn push_str(&mut self, chunk: &str) -> Vec<RpcModeOutput> {
        self.reader
            .push_str(chunk)
            .into_iter()
            .filter_map(|line| self.handle_line(&line))
            .collect()
    }

    pub fn finish(&mut self) -> Vec<RpcModeOutput> {
        self.reader
            .finish()
            .into_iter()
            .filter_map(|line| self.handle_line(&line))
            .collect()
    }

    pub fn handle_line(&mut self, line: &str) -> Option<RpcModeOutput> {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            return None;
        }

        let parsed = match serde_json::from_str::<serde_json::Value>(trimmed) {
            Ok(value) => value,
            Err(error) => {
                return Some(response_output(RpcResponse::error(
                    None,
                    "parse",
                    format!("Failed to parse command: {error}"),
                )));
            }
        };

        if is_extension_ui_response(&parsed) {
            let _ = serde_json::from_value::<RpcExtensionUiResponse>(parsed);
            return None;
        }

        let unknown_type = unknown_command_type(&parsed).map(ToString::to_string);
        match serde_json::from_value::<RpcCommand>(parsed) {
            Ok(command) => Some(response_output(self.dispatcher.handle_command(command))),
            Err(error) => {
                if let Some(command_type) = unknown_type {
                    return Some(response_output(RpcResponse::error(
                        None,
                        &command_type,
                        format!("Unknown command: {command_type}"),
                    )));
                }
                Some(response_output(RpcResponse::error(
                    None,
                    "parse",
                    format!("Failed to parse command: {error}"),
                )))
            }
        }
    }
}

fn response_output(response: RpcResponse) -> RpcModeOutput {
    let line = serialize_json_line(&response).unwrap_or_else(|error| {
        format!(
            "{{\"type\":\"response\",\"command\":\"serialize\",\"success\":false,\"error\":\"{}\"}}\n",
            escape_json_string(error.to_string().as_str())
        )
    });
    RpcModeOutput { line }
}

fn is_extension_ui_response(value: &serde_json::Value) -> bool {
    value
        .get("type")
        .and_then(serde_json::Value::as_str)
        .is_some_and(|value| value == "extension_ui_response")
}

fn unknown_command_type(value: &serde_json::Value) -> Option<&str> {
    let command_type = value.get("type").and_then(serde_json::Value::as_str)?;
    let known = [
        "prompt",
        "steer",
        "follow_up",
        "abort",
        "new_session",
        "get_state",
        "set_model",
        "cycle_model",
        "get_available_models",
        "set_thinking_level",
        "cycle_thinking_level",
        "set_steering_mode",
        "set_follow_up_mode",
        "compact",
        "set_auto_compaction",
        "set_auto_retry",
        "abort_retry",
        "bash",
        "abort_bash",
        "get_session_stats",
        "export_html",
        "switch_session",
        "fork",
        "clone",
        "get_fork_messages",
        "get_last_assistant_text",
        "set_session_name",
        "get_messages",
        "get_commands",
    ];
    (!known.contains(&command_type)).then_some(command_type)
}

fn escape_json_string(value: &str) -> String {
    value.replace('\\', "\\\\").replace('"', "\\\"")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rpc::dispatcher::RpcSessionBackend;
    use crate::rpc::types::{QueueMode, RpcSessionState};
    use agent::AgentMessage;
    use ai::{MessageRole, Model, ModelThinkingLevel};

    #[derive(Default)]
    struct TestBackend {
        messages: Vec<AgentMessage>,
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
                thinking_level: ModelThinkingLevel::Off,
                is_streaming: false,
                is_compacting: false,
                steering_mode: QueueMode::OneAtATime,
                follow_up_mode: QueueMode::OneAtATime,
                session_file: None,
                session_id: "session".to_string(),
                session_name: None,
                auto_compaction_enabled: true,
                message_count: self.messages.len(),
                pending_message_count: 0,
            })
        }

        fn set_model(&mut self, _provider: String, _model_id: String) -> Result<Model, String> {
            Err("not found".to_string())
        }

        fn available_models(&self) -> Result<Vec<Model>, String> {
            Ok(Vec::new())
        }

        fn last_assistant_text(&self) -> Result<Option<String>, String> {
            Ok(None)
        }

        fn messages(&self) -> Result<Vec<AgentMessage>, String> {
            Ok(self.messages.clone())
        }
    }

    #[test]
    fn handles_jsonl_command_chunks() {
        let mut mode = RpcMode::new(TestBackend::default());
        let outputs = mode.push_str("{\"type\":\"prompt\",\"id\":\"1\",\"message\":\"hello\"}\n");

        assert_eq!(outputs.len(), 1);
        assert!(outputs[0].line.contains("\"type\":\"response\""));
        assert!(outputs[0].line.contains("\"command\":\"prompt\""));
        assert!(outputs[0].line.contains("\"success\":true"));
    }

    #[test]
    fn reports_parse_errors_as_rpc_response() {
        let mut mode = RpcMode::new(TestBackend::default());
        let outputs = mode.push_str("{bad json}\n");

        assert_eq!(outputs.len(), 1);
        assert!(outputs[0].line.contains("\"command\":\"parse\""));
        assert!(outputs[0].line.contains("\"success\":false"));
    }

    #[test]
    fn ignores_extension_ui_responses_without_pending_request() {
        let mut mode = RpcMode::new(TestBackend::default());
        let outputs =
            mode.push_str("{\"type\":\"extension_ui_response\",\"id\":\"x\",\"value\":\"ok\"}\n");

        assert!(outputs.is_empty());
    }

    #[test]
    fn reports_unknown_commands_like_pi_rpc_mode() {
        let mut mode = RpcMode::new(TestBackend::default());
        let outputs = mode.push_str("{\"type\":\"missing_command\",\"id\":\"x\"}\n");

        assert_eq!(outputs.len(), 1);
        assert!(outputs[0].line.contains("\"command\":\"missing_command\""));
        assert!(outputs[0].line.contains("\"success\":false"));
        assert!(outputs[0]
            .line
            .contains("\"error\":\"Unknown command: missing_command\""));
        assert!(!outputs[0].line.contains("\"id\":\"x\""));
    }

    #[test]
    fn finishes_partial_line() {
        let mut mode = RpcMode::new(TestBackend::default());
        assert!(mode
            .push_str("{\"type\":\"get_state\",\"id\":\"s\"}")
            .is_empty());
        let outputs = mode.finish();

        assert_eq!(outputs.len(), 1);
        assert!(outputs[0].line.contains("\"command\":\"get_state\""));
    }
}
