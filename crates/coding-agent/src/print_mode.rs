use ai::{AssistantContentBlock, AssistantStopReason, RichAssistantMessage};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PrintTextOutput {
    pub stdout: String,
    pub stderr: String,
    pub exit_code: i32,
}

pub fn render_print_text_output(message: &RichAssistantMessage) -> PrintTextOutput {
    if matches!(
        message.stop_reason,
        AssistantStopReason::Error | AssistantStopReason::Aborted
    ) {
        let fallback = format!("Request {}", message.stop_reason.as_str());
        return PrintTextOutput {
            stdout: String::new(),
            stderr: format!(
                "{}\n",
                message.error_message.as_deref().unwrap_or(&fallback)
            ),
            exit_code: 1,
        };
    }

    let mut stdout = String::new();
    for content in &message.content {
        if let AssistantContentBlock::Text(text) = content {
            stdout.push_str(&text.text);
            stdout.push('\n');
        }
    }

    PrintTextOutput {
        stdout,
        stderr: String::new(),
        exit_code: 0,
    }
}

pub fn render_print_text_output_from_last_message(
    message: Option<&ai::RichMessage>,
) -> PrintTextOutput {
    match message {
        Some(ai::RichMessage::Assistant(message)) => render_print_text_output(message),
        _ => PrintTextOutput {
            stdout: String::new(),
            stderr: String::new(),
            exit_code: 0,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ai::{
        AssistantContentBlock, AssistantStopReason, RichAssistantMessage, TextContent, Usage,
    };

    fn assistant_message(
        content: Vec<AssistantContentBlock>,
        stop_reason: AssistantStopReason,
        error_message: Option<&str>,
    ) -> RichAssistantMessage {
        RichAssistantMessage {
            content,
            api: "openai-responses".to_string(),
            provider: "openai".to_string(),
            model: "gpt-4o-mini".to_string(),
            response_model: None,
            response_id: None,
            usage: Usage::default(),
            stop_reason,
            error_message: error_message.map(str::to_string),
            diagnostics: Vec::new(),
            timestamp_millis: 1,
        }
    }

    #[test]
    fn text_mode_extracts_text_blocks_like_pi_print_mode() {
        let result = render_print_text_output(&assistant_message(
            vec![
                AssistantContentBlock::Text(TextContent {
                    text: "first".to_string(),
                    text_signature: None,
                }),
                AssistantContentBlock::ToolCall(ai::ToolCall {
                    id: "call-1".to_string(),
                    name: "read".to_string(),
                    arguments: Default::default(),
                    thought_signature: None,
                }),
                AssistantContentBlock::Text(TextContent {
                    text: "second".to_string(),
                    text_signature: None,
                }),
            ],
            AssistantStopReason::Stop,
            None,
        ));

        assert_eq!(
            result,
            PrintTextOutput {
                stdout: "first\nsecond\n".to_string(),
                stderr: String::new(),
                exit_code: 0,
            }
        );
    }

    #[test]
    fn text_mode_returns_error_for_error_or_aborted_assistant_like_pi_print_mode() {
        assert_eq!(
            render_print_text_output(&assistant_message(
                Vec::new(),
                AssistantStopReason::Error,
                Some("provider failure"),
            )),
            PrintTextOutput {
                stdout: String::new(),
                stderr: "provider failure\n".to_string(),
                exit_code: 1,
            }
        );
        assert_eq!(
            render_print_text_output(&assistant_message(
                Vec::new(),
                AssistantStopReason::Aborted,
                None,
            )),
            PrintTextOutput {
                stdout: String::new(),
                stderr: "Request aborted\n".to_string(),
                exit_code: 1,
            }
        );
    }

    #[test]
    fn text_mode_is_silent_without_last_assistant_message_like_pi_print_mode() {
        assert_eq!(
            render_print_text_output_from_last_message(None),
            PrintTextOutput {
                stdout: String::new(),
                stderr: String::new(),
                exit_code: 0,
            }
        );
        assert_eq!(
            render_print_text_output_from_last_message(Some(&ai::RichMessage::User(
                ai::UserMessage {
                    content: ai::UserMessageContent::Text("hello".to_string()),
                    timestamp_millis: 1,
                },
            ))),
            PrintTextOutput {
                stdout: String::new(),
                stderr: String::new(),
                exit_code: 0,
            }
        );
    }
}
