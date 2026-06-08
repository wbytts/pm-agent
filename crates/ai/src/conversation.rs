use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::types::{AssistantStopReason, Model, Usage};
use crate::utils::{AssistantMessageDiagnostic, DiagnosticTarget};

pub const NON_VISION_USER_IMAGE_PLACEHOLDER: &str =
    "(image omitted: model does not support images)";
pub const NON_VISION_TOOL_IMAGE_PLACEHOLDER: &str =
    "(tool image omitted: model does not support images)";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct TextContent {
    pub text: String,
    #[serde(default)]
    pub text_signature: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ThinkingContent {
    pub thinking: String,
    #[serde(default)]
    pub thinking_signature: Option<String>,
    #[serde(default)]
    pub redacted: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ImageContent {
    pub data: String,
    pub mime_type: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ToolCall {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub arguments: BTreeMap<String, Value>,
    #[serde(default)]
    pub thought_signature: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum UserContentBlock {
    Text(TextContent),
    Image(ImageContent),
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum AssistantContentBlock {
    Text(TextContent),
    Thinking(ThinkingContent),
    ToolCall(ToolCall),
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum UserMessageContent {
    Text(String),
    Blocks(Vec<UserContentBlock>),
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct UserMessage {
    pub content: UserMessageContent,
    pub timestamp_millis: u128,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct RichAssistantMessage {
    pub content: Vec<AssistantContentBlock>,
    pub api: String,
    pub provider: String,
    pub model: String,
    #[serde(default)]
    pub response_model: Option<String>,
    #[serde(default)]
    pub response_id: Option<String>,
    pub usage: Usage,
    pub stop_reason: AssistantStopReason,
    #[serde(default)]
    pub error_message: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub diagnostics: Vec<AssistantMessageDiagnostic>,
    pub timestamp_millis: u128,
}

impl DiagnosticTarget for RichAssistantMessage {
    fn diagnostics_mut(&mut self) -> &mut Vec<AssistantMessageDiagnostic> {
        &mut self.diagnostics
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ToolResultMessage {
    pub tool_call_id: String,
    pub tool_name: String,
    pub content: Vec<UserContentBlock>,
    #[serde(default)]
    pub details: Option<Value>,
    pub is_error: bool,
    pub timestamp_millis: u128,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "role", rename_all = "camelCase")]
pub enum RichMessage {
    User(UserMessage),
    Assistant(RichAssistantMessage),
    ToolResult(ToolResultMessage),
}

pub fn transform_messages<F>(
    messages: &[RichMessage],
    model: &Model,
    normalize_tool_call_id: Option<F>,
) -> Vec<RichMessage>
where
    F: Fn(&str, &Model, &RichAssistantMessage) -> String,
{
    let mut tool_call_id_map: BTreeMap<String, String> = BTreeMap::new();
    let image_aware_messages = downgrade_unsupported_images(messages, model);
    let transformed = image_aware_messages
        .into_iter()
        .map(|message| match message {
            RichMessage::User(_) => message,
            RichMessage::ToolResult(mut tool_result) => {
                if let Some(normalized_id) = tool_call_id_map.get(&tool_result.tool_call_id) {
                    tool_result.tool_call_id = normalized_id.clone();
                }
                RichMessage::ToolResult(tool_result)
            }
            RichMessage::Assistant(mut assistant) => {
                let is_same_model = assistant.provider == model.provider
                    && assistant.api == model.api
                    && assistant.model == model.id;
                let mut next_content = Vec::new();

                let original_content = std::mem::take(&mut assistant.content);
                for block in original_content {
                    match block {
                        AssistantContentBlock::Thinking(thinking) => {
                            if thinking.redacted {
                                if is_same_model {
                                    next_content.push(AssistantContentBlock::Thinking(thinking));
                                }
                                continue;
                            }
                            if is_same_model && thinking.thinking_signature.is_some() {
                                next_content.push(AssistantContentBlock::Thinking(thinking));
                                continue;
                            }
                            if thinking.thinking.trim().is_empty() {
                                continue;
                            }
                            if is_same_model {
                                next_content.push(AssistantContentBlock::Thinking(thinking));
                            } else {
                                next_content.push(AssistantContentBlock::Text(TextContent {
                                    text: thinking.thinking,
                                    text_signature: None,
                                }));
                            }
                        }
                        AssistantContentBlock::Text(mut text) => {
                            if !is_same_model {
                                text.text_signature = None;
                            }
                            next_content.push(AssistantContentBlock::Text(text));
                        }
                        AssistantContentBlock::ToolCall(mut tool_call) => {
                            if !is_same_model {
                                tool_call.thought_signature = None;
                                if let Some(normalize_tool_call_id) =
                                    normalize_tool_call_id.as_ref()
                                {
                                    let normalized_id =
                                        normalize_tool_call_id(&tool_call.id, model, &assistant);
                                    if normalized_id != tool_call.id {
                                        tool_call_id_map
                                            .insert(tool_call.id.clone(), normalized_id.clone());
                                        tool_call.id = normalized_id;
                                    }
                                }
                            }
                            next_content.push(AssistantContentBlock::ToolCall(tool_call));
                        }
                    }
                }

                assistant.content = next_content;
                RichMessage::Assistant(assistant)
            }
        })
        .collect::<Vec<_>>();

    insert_synthetic_tool_results(transformed)
}

fn downgrade_unsupported_images(messages: &[RichMessage], model: &Model) -> Vec<RichMessage> {
    if model
        .input
        .iter()
        .any(|kind| *kind == crate::types::ModelInputKind::Image)
    {
        return messages.to_vec();
    }

    messages
        .iter()
        .cloned()
        .map(|message| match message {
            RichMessage::User(mut user) => {
                if let UserMessageContent::Blocks(blocks) = user.content {
                    user.content = UserMessageContent::Blocks(replace_images_with_placeholder(
                        blocks,
                        NON_VISION_USER_IMAGE_PLACEHOLDER,
                    ));
                }
                RichMessage::User(user)
            }
            RichMessage::ToolResult(mut tool_result) => {
                tool_result.content = replace_images_with_placeholder(
                    tool_result.content,
                    NON_VISION_TOOL_IMAGE_PLACEHOLDER,
                );
                RichMessage::ToolResult(tool_result)
            }
            other => other,
        })
        .collect()
}

fn replace_images_with_placeholder(
    content: Vec<UserContentBlock>,
    placeholder: &str,
) -> Vec<UserContentBlock> {
    let mut result = Vec::new();
    let mut previous_was_placeholder = false;

    for block in content {
        match block {
            UserContentBlock::Image(_) => {
                if !previous_was_placeholder {
                    result.push(UserContentBlock::Text(TextContent {
                        text: placeholder.to_string(),
                        text_signature: None,
                    }));
                }
                previous_was_placeholder = true;
            }
            UserContentBlock::Text(text) => {
                previous_was_placeholder = text.text == placeholder;
                result.push(UserContentBlock::Text(text));
            }
        }
    }

    result
}

fn insert_synthetic_tool_results(messages: Vec<RichMessage>) -> Vec<RichMessage> {
    let mut result = Vec::new();
    let mut pending_tool_calls: Vec<ToolCall> = Vec::new();
    let mut existing_tool_result_ids = Vec::new();

    for message in messages {
        match message {
            RichMessage::Assistant(assistant) => {
                insert_pending_tool_results(
                    &mut result,
                    &mut pending_tool_calls,
                    &mut existing_tool_result_ids,
                );

                if matches!(
                    assistant.stop_reason,
                    AssistantStopReason::Error | AssistantStopReason::Aborted
                ) {
                    continue;
                }

                pending_tool_calls = assistant
                    .content
                    .iter()
                    .filter_map(|block| match block {
                        AssistantContentBlock::ToolCall(tool_call) => Some(tool_call.clone()),
                        _ => None,
                    })
                    .collect();
                existing_tool_result_ids.clear();
                result.push(RichMessage::Assistant(assistant));
            }
            RichMessage::ToolResult(tool_result) => {
                existing_tool_result_ids.push(tool_result.tool_call_id.clone());
                result.push(RichMessage::ToolResult(tool_result));
            }
            RichMessage::User(user) => {
                insert_pending_tool_results(
                    &mut result,
                    &mut pending_tool_calls,
                    &mut existing_tool_result_ids,
                );
                result.push(RichMessage::User(user));
            }
        }
    }

    insert_pending_tool_results(
        &mut result,
        &mut pending_tool_calls,
        &mut existing_tool_result_ids,
    );
    result
}

fn insert_pending_tool_results(
    result: &mut Vec<RichMessage>,
    pending_tool_calls: &mut Vec<ToolCall>,
    existing_tool_result_ids: &mut Vec<String>,
) {
    if pending_tool_calls.is_empty() {
        return;
    }

    for tool_call in pending_tool_calls.drain(..) {
        if existing_tool_result_ids.contains(&tool_call.id) {
            continue;
        }
        result.push(RichMessage::ToolResult(ToolResultMessage {
            tool_call_id: tool_call.id,
            tool_name: tool_call.name,
            content: vec![UserContentBlock::Text(TextContent {
                text: "No result provided".to_string(),
                text_signature: None,
            })],
            details: None,
            is_error: true,
            timestamp_millis: 0,
        }));
    }
    existing_tool_result_ids.clear();
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{ModelInputKind, UsageCost};

    #[test]
    fn downgrades_images_for_non_vision_models() {
        let result = transform_messages(
            &[RichMessage::User(UserMessage {
                content: UserMessageContent::Blocks(vec![
                    UserContentBlock::Text(text("hello")),
                    UserContentBlock::Image(ImageContent {
                        data: "abc".to_string(),
                        mime_type: "image/png".to_string(),
                    }),
                    UserContentBlock::Image(ImageContent {
                        data: "def".to_string(),
                        mime_type: "image/png".to_string(),
                    }),
                ]),
                timestamp_millis: 1,
            })],
            &model_without_images(),
            None::<fn(&str, &Model, &RichAssistantMessage) -> String>,
        );

        assert_eq!(
            result,
            vec![RichMessage::User(UserMessage {
                content: UserMessageContent::Blocks(vec![
                    UserContentBlock::Text(text("hello")),
                    UserContentBlock::Text(text(NON_VISION_USER_IMAGE_PLACEHOLDER)),
                ]),
                timestamp_millis: 1,
            })]
        );
    }

    #[test]
    fn preserves_images_for_vision_models() {
        let image = UserContentBlock::Image(ImageContent {
            data: "abc".to_string(),
            mime_type: "image/png".to_string(),
        });
        let result = transform_messages(
            &[RichMessage::User(UserMessage {
                content: UserMessageContent::Blocks(vec![image.clone()]),
                timestamp_millis: 1,
            })],
            &model_with_images(),
            None::<fn(&str, &Model, &RichAssistantMessage) -> String>,
        );

        assert_eq!(
            result,
            vec![RichMessage::User(UserMessage {
                content: UserMessageContent::Blocks(vec![image]),
                timestamp_millis: 1,
            })]
        );
    }

    #[test]
    fn converts_cross_model_thinking_to_text_and_drops_signatures() {
        let result = transform_messages(
            &[RichMessage::Assistant(RichAssistantMessage {
                content: vec![
                    AssistantContentBlock::Thinking(ThinkingContent {
                        thinking: "private reasoning".to_string(),
                        thinking_signature: Some("sig".to_string()),
                        redacted: false,
                    }),
                    AssistantContentBlock::Text(TextContent {
                        text: "answer".to_string(),
                        text_signature: Some("text-sig".to_string()),
                    }),
                ],
                provider: "other".to_string(),
                api: "other-api".to_string(),
                model: "other-model".to_string(),
                ..assistant_defaults()
            })],
            &model_without_images(),
            None::<fn(&str, &Model, &RichAssistantMessage) -> String>,
        );

        let RichMessage::Assistant(assistant) = &result[0] else {
            panic!("assistant message");
        };
        assert_eq!(
            assistant.content,
            vec![
                AssistantContentBlock::Text(text("private reasoning")),
                AssistantContentBlock::Text(text("answer")),
            ]
        );
    }

    #[test]
    fn keeps_same_model_redacted_thinking_and_drops_foreign_redacted_thinking() {
        let same_model = RichMessage::Assistant(RichAssistantMessage {
            content: vec![AssistantContentBlock::Thinking(ThinkingContent {
                thinking: String::new(),
                thinking_signature: Some("opaque".to_string()),
                redacted: true,
            })],
            ..assistant_defaults()
        });
        let foreign_model = RichMessage::Assistant(RichAssistantMessage {
            content: vec![AssistantContentBlock::Thinking(ThinkingContent {
                thinking: String::new(),
                thinking_signature: Some("opaque".to_string()),
                redacted: true,
            })],
            provider: "other".to_string(),
            ..assistant_defaults()
        });

        let same_result = transform_messages(
            &[same_model],
            &model_without_images(),
            None::<fn(&str, &Model, &RichAssistantMessage) -> String>,
        );
        let foreign_result = transform_messages(
            &[foreign_model],
            &model_without_images(),
            None::<fn(&str, &Model, &RichAssistantMessage) -> String>,
        );

        let RichMessage::Assistant(same_assistant) = &same_result[0] else {
            panic!("assistant message");
        };
        let RichMessage::Assistant(foreign_assistant) = &foreign_result[0] else {
            panic!("assistant message");
        };
        assert_eq!(same_assistant.content.len(), 1);
        assert!(foreign_assistant.content.is_empty());
    }

    #[test]
    fn normalizes_cross_model_tool_call_id_and_matching_result() {
        let messages = vec![
            RichMessage::Assistant(RichAssistantMessage {
                content: vec![AssistantContentBlock::ToolCall(ToolCall {
                    id: "call|foreign item".to_string(),
                    name: "read_file".to_string(),
                    arguments: BTreeMap::new(),
                    thought_signature: Some("sig".to_string()),
                })],
                provider: "other".to_string(),
                ..assistant_defaults()
            }),
            RichMessage::ToolResult(ToolResultMessage {
                tool_call_id: "call|foreign item".to_string(),
                tool_name: "read_file".to_string(),
                content: vec![UserContentBlock::Text(text("ok"))],
                details: None,
                is_error: false,
                timestamp_millis: 2,
            }),
        ];

        let result = transform_messages(
            &messages,
            &model_without_images(),
            Some(|id: &str, _model: &Model, _source: &RichAssistantMessage| {
                id.replace('|', "_").replace(' ', "_")
            }),
        );

        let RichMessage::Assistant(assistant) = &result[0] else {
            panic!("assistant message");
        };
        let RichMessage::ToolResult(tool_result) = &result[1] else {
            panic!("tool result");
        };
        let AssistantContentBlock::ToolCall(tool_call) = &assistant.content[0] else {
            panic!("tool call");
        };

        assert_eq!(tool_call.id, "call_foreign_item");
        assert_eq!(tool_call.thought_signature, None);
        assert_eq!(tool_result.tool_call_id, "call_foreign_item");
    }

    #[test]
    fn inserts_synthetic_tool_results_for_orphaned_tool_calls() {
        let result = transform_messages(
            &[RichMessage::Assistant(RichAssistantMessage {
                content: vec![AssistantContentBlock::ToolCall(ToolCall {
                    id: "call_1".to_string(),
                    name: "read_file".to_string(),
                    arguments: BTreeMap::new(),
                    thought_signature: None,
                })],
                ..assistant_defaults()
            })],
            &model_without_images(),
            None::<fn(&str, &Model, &RichAssistantMessage) -> String>,
        );

        assert_eq!(result.len(), 2);
        let RichMessage::ToolResult(tool_result) = &result[1] else {
            panic!("tool result");
        };
        assert_eq!(tool_result.tool_call_id, "call_1");
        assert_eq!(tool_result.tool_name, "read_file");
        assert!(tool_result.is_error);
    }

    #[test]
    fn skips_error_assistant_messages() {
        let result = transform_messages(
            &[RichMessage::Assistant(RichAssistantMessage {
                stop_reason: AssistantStopReason::Error,
                error_message: Some("failed".to_string()),
                ..assistant_defaults()
            })],
            &model_without_images(),
            None::<fn(&str, &Model, &RichAssistantMessage) -> String>,
        );

        assert!(result.is_empty());
    }

    fn text(value: &str) -> TextContent {
        TextContent {
            text: value.to_string(),
            text_signature: None,
        }
    }

    fn assistant_defaults() -> RichAssistantMessage {
        RichAssistantMessage {
            content: Vec::new(),
            api: "openai-responses".to_string(),
            provider: "openai".to_string(),
            model: "gpt-5".to_string(),
            response_model: None,
            response_id: None,
            usage: Usage {
                input: 0,
                output: 0,
                cache_read: 0,
                cache_write: 0,
                total_tokens: 0,
                cost: UsageCost::default(),
            },
            stop_reason: AssistantStopReason::Stop,
            error_message: None,
            diagnostics: Vec::new(),
            timestamp_millis: 1,
        }
    }

    fn model_without_images() -> Model {
        Model {
            id: "gpt-5".to_string(),
            provider: "openai".to_string(),
            api: "openai-responses".to_string(),
            display_name: "GPT-5".to_string(),
            context_window: 1,
            input: vec![ModelInputKind::Text],
            ..Model::default()
        }
    }

    fn model_with_images() -> Model {
        Model {
            input: vec![ModelInputKind::Text, ModelInputKind::Image],
            ..model_without_images()
        }
    }
}
