use std::collections::BTreeMap;

use crate::conversation::{RichMessage, UserContentBlock, UserMessageContent};
use crate::types::{ContentBlock, Message, MessageRole};

pub fn infer_copilot_initiator(messages: &[Message]) -> &'static str {
    messages
        .last()
        .map(|message| {
            if message.role == MessageRole::User {
                "user"
            } else {
                "agent"
            }
        })
        .unwrap_or("user")
}

pub fn has_copilot_vision_input(content: &[ContentBlock]) -> bool {
    content
        .iter()
        .any(|block| matches!(block, ContentBlock::Image { .. }))
}

pub fn has_copilot_vision_messages(messages: &[RichMessage]) -> bool {
    messages.iter().any(|message| match message {
        RichMessage::User(user) => match &user.content {
            UserMessageContent::Blocks(blocks) => has_user_content_image(blocks),
            UserMessageContent::Text(_) => false,
        },
        RichMessage::ToolResult(tool_result) => has_user_content_image(&tool_result.content),
        RichMessage::Assistant(_) => false,
    })
}

fn has_user_content_image(content: &[UserContentBlock]) -> bool {
    content
        .iter()
        .any(|block| matches!(block, UserContentBlock::Image(_)))
}

pub fn build_copilot_dynamic_headers(
    messages: &[Message],
    has_images: bool,
) -> BTreeMap<String, String> {
    let mut headers = BTreeMap::from([
        (
            "X-Initiator".to_string(),
            infer_copilot_initiator(messages).to_string(),
        ),
        (
            "Openai-Intent".to_string(),
            "conversation-edits".to_string(),
        ),
    ]);

    if has_images {
        headers.insert("Copilot-Vision-Request".to_string(), "true".to_string());
    }

    headers
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::conversation::{
        ImageContent, RichAssistantMessage, TextContent, ToolCall, ToolResultMessage, UserMessage,
        UserMessageContent,
    };
    use crate::types::{AssistantStopReason, Usage};
    use std::collections::BTreeMap;

    #[test]
    fn infers_user_initiator_for_empty_or_user_last_message() {
        assert_eq!(infer_copilot_initiator(&[]), "user");
        assert_eq!(
            infer_copilot_initiator(&[Message {
                role: MessageRole::User,
                content: "hello".to_string(),
            }]),
            "user"
        );
    }

    #[test]
    fn infers_agent_initiator_after_non_user_message() {
        assert_eq!(
            infer_copilot_initiator(&[Message {
                role: MessageRole::Assistant,
                content: "hello".to_string(),
            }]),
            "agent"
        );
    }

    #[test]
    fn detects_vision_content() {
        assert!(has_copilot_vision_input(&[ContentBlock::Image {
            data: "abc".to_string(),
            mime_type: "image/png".to_string(),
        }]));
        assert!(!has_copilot_vision_input(&[ContentBlock::Text {
            text: "abc".to_string(),
        }]));
    }

    #[test]
    fn detects_vision_messages_like_pi_copilot_headers() {
        let image = UserContentBlock::Image(ImageContent {
            data: "abc".to_string(),
            mime_type: "image/png".to_string(),
        });
        let text = UserContentBlock::Text(TextContent {
            text: "hello".to_string(),
            text_signature: None,
        });

        assert!(has_copilot_vision_messages(&[RichMessage::User(
            UserMessage {
                content: UserMessageContent::Blocks(vec![text.clone(), image.clone()]),
                timestamp_millis: 1,
            }
        )]));
        assert!(has_copilot_vision_messages(&[RichMessage::ToolResult(
            ToolResultMessage {
                tool_call_id: "call-1".to_string(),
                tool_name: "read".to_string(),
                content: vec![image],
                details: None,
                is_error: false,
                timestamp_millis: 2,
            }
        )]));
        assert!(!has_copilot_vision_messages(&[RichMessage::Assistant(
            RichAssistantMessage {
                content: vec![crate::conversation::AssistantContentBlock::ToolCall(
                    ToolCall {
                        id: "call-1".to_string(),
                        name: "read".to_string(),
                        arguments: BTreeMap::new(),
                        thought_signature: None,
                    },
                )],
                api: "openai-responses".to_string(),
                provider: "github-copilot".to_string(),
                model: "gpt-5.4".to_string(),
                response_model: None,
                response_id: None,
                usage: Usage::default(),
                stop_reason: AssistantStopReason::ToolUse,
                error_message: None,
                diagnostics: Vec::new(),
                timestamp_millis: 3,
            }
        )]));
    }

    #[test]
    fn builds_dynamic_headers() {
        let headers = build_copilot_dynamic_headers(
            &[Message {
                role: MessageRole::Assistant,
                content: "hello".to_string(),
            }],
            true,
        );

        assert_eq!(
            headers.get("X-Initiator").map(String::as_str),
            Some("agent")
        );
        assert_eq!(
            headers.get("Openai-Intent").map(String::as_str),
            Some("conversation-edits")
        );
        assert_eq!(
            headers.get("Copilot-Vision-Request").map(String::as_str),
            Some("true")
        );
    }
}
