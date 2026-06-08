use std::collections::BTreeMap;

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
