use agent::harness::SessionStorage;

use crate::session_manager::SessionManager;
use crate::utils::{
    copy_to_clipboard, copy_to_clipboard_with_runner, ClipboardEnvironment, ClipboardError,
    ClipboardPlatform, ClipboardRunner,
};

pub const NO_AGENT_MESSAGES_TO_COPY: &str = "No agent messages to copy yet.";

pub fn copy_last_assistant_text<S: SessionStorage>(
    manager: &SessionManager<S>,
) -> Result<String, CopyLastAssistantTextError> {
    let text = last_assistant_text(manager)?;
    copy_to_clipboard(&text).map_err(CopyLastAssistantTextError::Clipboard)?;
    Ok(text)
}

pub fn copy_last_assistant_text_with_runner<S: SessionStorage>(
    manager: &SessionManager<S>,
    platform: ClipboardPlatform,
    environment: ClipboardEnvironment,
    remote: bool,
    runner: &mut dyn ClipboardRunner,
) -> Result<String, CopyLastAssistantTextError> {
    let text = last_assistant_text(manager)?;
    copy_to_clipboard_with_runner(&text, platform, environment, remote, runner)
        .map_err(CopyLastAssistantTextError::Clipboard)?;
    Ok(text)
}

fn last_assistant_text<S: SessionStorage>(
    manager: &SessionManager<S>,
) -> Result<String, CopyLastAssistantTextError> {
    manager
        .last_assistant_text()
        .ok_or(CopyLastAssistantTextError::NoAgentMessages)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CopyLastAssistantTextError {
    NoAgentMessages,
    Clipboard(ClipboardError),
}

impl std::fmt::Display for CopyLastAssistantTextError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NoAgentMessages => formatter.write_str(NO_AGENT_MESSAGES_TO_COPY),
            Self::Clipboard(error) => write!(formatter, "{error:?}"),
        }
    }
}

impl std::error::Error for CopyLastAssistantTextError {}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::utils::{ClipboardCommand, ClipboardCommandMode};
    use agent::AgentMessage;
    use ai::MessageRole;

    #[derive(Debug, Default)]
    struct RecordingClipboardRunner {
        copied: Vec<String>,
    }

    impl ClipboardRunner for RecordingClipboardRunner {
        fn run_command(&mut self, _command: &ClipboardCommand, text: &str) -> bool {
            self.copied.push(text.to_string());
            true
        }

        fn emit_osc52(&mut self, _sequence: &str) -> bool {
            false
        }
    }

    #[test]
    fn copies_last_assistant_text_like_pi_copy_command() {
        let mut manager = SessionManager::in_memory("/tmp/project");
        manager
            .append_message(AgentMessage::new(
                MessageRole::Assistant,
                "older answer".to_string(),
            ))
            .expect("older answer");
        manager
            .append_message(AgentMessage::new(MessageRole::User, "question".to_string()))
            .expect("question");
        manager
            .append_message(AgentMessage::new(
                MessageRole::Assistant,
                "latest answer".to_string(),
            ))
            .expect("latest answer");
        let mut runner = RecordingClipboardRunner::default();

        let copied = copy_last_assistant_text_with_runner(
            &manager,
            ClipboardPlatform::Macos,
            ClipboardEnvironment::default(),
            false,
            &mut runner,
        )
        .expect("copy");

        assert_eq!(copied, "latest answer");
        assert_eq!(runner.copied, vec!["latest answer".to_string()]);
    }

    #[test]
    fn reports_pi_no_agent_messages_error_without_copying() {
        let mut manager = SessionManager::in_memory("/tmp/project");
        manager
            .append_message(AgentMessage::new(MessageRole::User, "question".to_string()))
            .expect("question");
        let mut runner = RecordingClipboardRunner::default();

        let error = copy_last_assistant_text_with_runner(
            &manager,
            ClipboardPlatform::Macos,
            ClipboardEnvironment::default(),
            false,
            &mut runner,
        )
        .expect_err("copy should fail");

        assert_eq!(error, CopyLastAssistantTextError::NoAgentMessages);
        assert_eq!(error.to_string(), NO_AGENT_MESSAGES_TO_COPY);
        assert!(runner.copied.is_empty());
    }

    #[test]
    fn maps_clipboard_failures() {
        struct FailingRunner;

        impl ClipboardRunner for FailingRunner {
            fn run_command(&mut self, _command: &ClipboardCommand, _text: &str) -> bool {
                false
            }

            fn emit_osc52(&mut self, _sequence: &str) -> bool {
                false
            }
        }

        let mut manager = SessionManager::in_memory("/tmp/project");
        manager
            .append_message(AgentMessage::new(
                MessageRole::Assistant,
                "answer".to_string(),
            ))
            .expect("answer");
        let mut runner = FailingRunner;

        let error = copy_last_assistant_text_with_runner(
            &manager,
            ClipboardPlatform::Other,
            ClipboardEnvironment::default(),
            false,
            &mut runner,
        )
        .expect_err("copy should fail");

        assert_eq!(
            error,
            CopyLastAssistantTextError::Clipboard(ClipboardError::CopyFailed)
        );
    }

    #[test]
    fn test_runner_uses_command_mode() {
        let command = ClipboardCommand {
            mode: ClipboardCommandMode::Exec,
            command: "pbcopy".to_string(),
        };
        let mut runner = RecordingClipboardRunner::default();

        assert!(runner.run_command(&command, "text"));

        assert_eq!(runner.copied, vec!["text".to_string()]);
    }
}
