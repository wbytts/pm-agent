use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use crate::conversation::{
    transform_messages, AssistantContentBlock, RichMessage, TextContent, UserContentBlock,
    UserMessageContent,
};
use crate::types::{AssistantStopReason, Model, ModelInputKind};
use crate::utils::sanitize_surrogates;

pub const GOOGLE_JSON_SCHEMA_META_DECLARATIONS: &[&str] = &[
    "$schema",
    "$id",
    "$anchor",
    "$dynamicAnchor",
    "$vocabulary",
    "$comment",
    "$defs",
    "definitions",
];

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum GoogleThinkingLevel {
    ThinkingLevelUnspecified,
    Minimal,
    Low,
    Medium,
    High,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GooglePartThinkingState {
    pub thought: bool,
    pub thought_signature: Option<String>,
}

pub fn is_thinking_part(part: &GooglePartThinkingState) -> bool {
    part.thought
}

pub fn retain_thought_signature(existing: Option<&str>, incoming: Option<&str>) -> Option<String> {
    incoming
        .filter(|value| !value.is_empty())
        .or(existing)
        .map(str::to_string)
}

pub fn requires_google_tool_call_id(model_id: &str) -> bool {
    model_id.starts_with("claude-") || model_id.starts_with("gpt-oss-")
}

pub fn gemini_major_version(model_id: &str) -> Option<u32> {
    let lower = model_id.to_ascii_lowercase();
    let version = lower
        .strip_prefix("gemini-live-")
        .or_else(|| lower.strip_prefix("gemini-"))?;
    let major: String = version
        .chars()
        .take_while(|character| character.is_ascii_digit())
        .collect();
    if major.is_empty() {
        return None;
    }
    major.parse::<u32>().ok()
}

pub fn supports_multimodal_function_response(model_id: &str) -> bool {
    gemini_major_version(model_id).map_or(true, |version| version >= 3)
}

pub fn resolve_google_thought_signature(
    is_same_provider_and_model: bool,
    signature: Option<&str>,
) -> Option<String> {
    let signature = signature?;
    if is_same_provider_and_model && is_valid_google_thought_signature(signature) {
        Some(signature.to_string())
    } else {
        None
    }
}

pub fn is_valid_google_thought_signature(signature: &str) -> bool {
    if signature.is_empty() || !signature.len().is_multiple_of(4) {
        return false;
    }
    signature
        .bytes()
        .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'+' | b'/' | b'='))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GoogleToolChoiceMode {
    Auto,
    None,
    Any,
}

pub fn map_google_tool_choice(choice: &str) -> GoogleToolChoiceMode {
    match choice {
        "none" => GoogleToolChoiceMode::None,
        "any" => GoogleToolChoiceMode::Any,
        "auto" => GoogleToolChoiceMode::Auto,
        _ => GoogleToolChoiceMode::Auto,
    }
}

pub fn map_google_stop_reason(reason: &str) -> AssistantStopReason {
    match reason {
        "STOP" => AssistantStopReason::Stop,
        "MAX_TOKENS" => AssistantStopReason::Length,
        _ => AssistantStopReason::Error,
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct GoogleContent {
    pub role: String,
    pub parts: Vec<GooglePart>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct GooglePart {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub inline_data: Option<GoogleInlineData>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub thought: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub thought_signature: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub function_call: Option<GoogleFunctionCall>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub function_response: Option<GoogleFunctionResponse>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct GoogleInlineData {
    pub mime_type: String,
    pub data: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct GoogleFunctionCall {
    pub name: String,
    #[serde(default)]
    pub args: BTreeMap<String, Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct GoogleFunctionResponse {
    pub name: String,
    pub response: BTreeMap<String, Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parts: Option<Vec<GooglePart>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct GoogleMessagesContext {
    pub messages: Vec<RichMessage>,
}

pub fn convert_google_messages(
    model: &Model,
    context: &GoogleMessagesContext,
) -> Vec<GoogleContent> {
    let transformed_messages = transform_messages(
        &context.messages,
        model,
        Some(normalize_google_tool_call_id_for_model),
    );
    let mut contents = Vec::new();

    for message in transformed_messages {
        match message {
            RichMessage::User(user) => {
                if let Some(parts) = google_parts_from_user_content(user.content) {
                    contents.push(GoogleContent {
                        role: "user".to_string(),
                        parts,
                    });
                }
            }
            RichMessage::Assistant(assistant) => {
                let is_same_provider_and_model =
                    assistant.provider == model.provider && assistant.model == model.id;
                let mut parts = Vec::new();

                for block in assistant.content {
                    match block {
                        AssistantContentBlock::Text(text) => {
                            if text.text.trim().is_empty() {
                                continue;
                            }
                            parts.push(GooglePart {
                                text: Some(sanitize_surrogates(&text.text)),
                                inline_data: None,
                                thought: None,
                                thought_signature: resolve_google_thought_signature(
                                    is_same_provider_and_model,
                                    text.text_signature.as_deref(),
                                ),
                                function_call: None,
                                function_response: None,
                            });
                        }
                        AssistantContentBlock::Thinking(thinking) => {
                            if thinking.thinking.trim().is_empty() {
                                continue;
                            }
                            let thought_signature = resolve_google_thought_signature(
                                is_same_provider_and_model,
                                thinking.thinking_signature.as_deref(),
                            );
                            parts.push(GooglePart {
                                text: Some(sanitize_surrogates(&thinking.thinking)),
                                inline_data: None,
                                thought: if is_same_provider_and_model {
                                    Some(true)
                                } else {
                                    None
                                },
                                thought_signature,
                                function_call: None,
                                function_response: None,
                            });
                        }
                        AssistantContentBlock::ToolCall(tool_call) => {
                            parts.push(GooglePart {
                                text: None,
                                inline_data: None,
                                thought: None,
                                thought_signature: resolve_google_thought_signature(
                                    is_same_provider_and_model,
                                    tool_call.thought_signature.as_deref(),
                                ),
                                function_call: Some(GoogleFunctionCall {
                                    name: tool_call.name,
                                    args: tool_call.arguments,
                                    id: requires_google_tool_call_id(&model.id)
                                        .then_some(tool_call.id),
                                }),
                                function_response: None,
                            });
                        }
                    }
                }

                if !parts.is_empty() {
                    contents.push(GoogleContent {
                        role: "model".to_string(),
                        parts,
                    });
                }
            }
            RichMessage::ToolResult(tool_result) => {
                let text_result = tool_result
                    .content
                    .iter()
                    .filter_map(|block| match block {
                        UserContentBlock::Text(TextContent { text, .. }) => Some(text.as_str()),
                        UserContentBlock::Image(_) => None,
                    })
                    .collect::<Vec<_>>()
                    .join("\n");
                let image_content = if model_supports_images(model) {
                    tool_result
                        .content
                        .iter()
                        .filter_map(|block| match block {
                            UserContentBlock::Image(image) => Some(image.clone()),
                            UserContentBlock::Text(_) => None,
                        })
                        .collect::<Vec<_>>()
                } else {
                    Vec::new()
                };
                let has_text = !text_result.is_empty();
                let has_images = !image_content.is_empty();
                let response_value = if has_text {
                    sanitize_surrogates(&text_result)
                } else if has_images {
                    "(see attached image)".to_string()
                } else {
                    String::new()
                };
                let image_parts = image_content
                    .into_iter()
                    .map(|image| GooglePart {
                        text: None,
                        inline_data: Some(GoogleInlineData {
                            mime_type: image.mime_type,
                            data: image.data,
                        }),
                        thought: None,
                        thought_signature: None,
                        function_call: None,
                        function_response: None,
                    })
                    .collect::<Vec<_>>();
                let supports_multimodal_response = supports_multimodal_function_response(&model.id);
                let mut response = BTreeMap::new();
                response.insert(
                    if tool_result.is_error {
                        "error"
                    } else {
                        "output"
                    }
                    .to_string(),
                    Value::String(response_value),
                );
                let function_response_part = GooglePart {
                    text: None,
                    inline_data: None,
                    thought: None,
                    thought_signature: None,
                    function_call: None,
                    function_response: Some(GoogleFunctionResponse {
                        name: tool_result.tool_name,
                        response,
                        parts: (has_images && supports_multimodal_response)
                            .then_some(image_parts.clone()),
                        id: requires_google_tool_call_id(&model.id)
                            .then_some(tool_result.tool_call_id),
                    }),
                };

                if let Some(last_content) = contents.last_mut() {
                    if last_content.role == "user"
                        && last_content
                            .parts
                            .iter()
                            .any(|part| part.function_response.is_some())
                    {
                        last_content.parts.push(function_response_part);
                    } else {
                        contents.push(GoogleContent {
                            role: "user".to_string(),
                            parts: vec![function_response_part],
                        });
                    }
                } else {
                    contents.push(GoogleContent {
                        role: "user".to_string(),
                        parts: vec![function_response_part],
                    });
                }

                if has_images && !supports_multimodal_response {
                    let mut parts = vec![GooglePart {
                        text: Some("Tool result image:".to_string()),
                        inline_data: None,
                        thought: None,
                        thought_signature: None,
                        function_call: None,
                        function_response: None,
                    }];
                    parts.extend(image_parts);
                    contents.push(GoogleContent {
                        role: "user".to_string(),
                        parts,
                    });
                }
            }
        }
    }

    contents
}

fn google_parts_from_user_content(content: UserMessageContent) -> Option<Vec<GooglePart>> {
    match content {
        UserMessageContent::Text(text) => Some(vec![GooglePart {
            text: Some(sanitize_surrogates(&text)),
            inline_data: None,
            thought: None,
            thought_signature: None,
            function_call: None,
            function_response: None,
        }]),
        UserMessageContent::Blocks(blocks) => {
            let parts = blocks
                .into_iter()
                .map(|block| match block {
                    UserContentBlock::Text(text) => GooglePart {
                        text: Some(sanitize_surrogates(&text.text)),
                        inline_data: None,
                        thought: None,
                        thought_signature: None,
                        function_call: None,
                        function_response: None,
                    },
                    UserContentBlock::Image(image) => GooglePart {
                        text: None,
                        inline_data: Some(GoogleInlineData {
                            mime_type: image.mime_type,
                            data: image.data,
                        }),
                        thought: None,
                        thought_signature: None,
                        function_call: None,
                        function_response: None,
                    },
                })
                .collect::<Vec<_>>();
            if parts.is_empty() {
                None
            } else {
                Some(parts)
            }
        }
    }
}

fn normalize_google_tool_call_id(id: &str) -> String {
    id.chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() || matches!(character, '_' | '-') {
                character
            } else {
                '_'
            }
        })
        .take(64)
        .collect()
}

fn normalize_google_tool_call_id_for_model(
    id: &str,
    target_model: &Model,
    _source: &crate::conversation::RichAssistantMessage,
) -> String {
    if !requires_google_tool_call_id(&target_model.id) {
        return id.to_string();
    }
    normalize_google_tool_call_id(id)
}

fn model_supports_images(model: &Model) -> bool {
    model
        .input
        .iter()
        .any(|kind| matches!(kind, ModelInputKind::Image))
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct GoogleToolDefinition {
    pub name: String,
    pub description: String,
    pub parameters: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct GoogleToolConfig {
    pub function_declarations: Vec<GoogleFunctionDeclaration>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct GoogleFunctionDeclaration {
    pub name: String,
    pub description: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parameters_json_schema: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parameters: Option<Value>,
}

pub fn sanitize_google_schema_for_open_api(schema: &Value) -> Value {
    match schema {
        Value::Object(map) => {
            let sanitized = map
                .iter()
                .filter(|(key, _)| !GOOGLE_JSON_SCHEMA_META_DECLARATIONS.contains(&key.as_str()))
                .map(|(key, value)| (key.clone(), sanitize_google_schema_for_open_api(value)))
                .collect::<Map<String, Value>>();
            Value::Object(sanitized)
        }
        Value::Array(_) => schema.clone(),
        _ => schema.clone(),
    }
}

pub fn convert_google_tools(
    tools: &[GoogleToolDefinition],
    use_parameters: bool,
) -> Option<Vec<GoogleToolConfig>> {
    if tools.is_empty() {
        return None;
    }

    Some(vec![GoogleToolConfig {
        function_declarations: tools
            .iter()
            .map(|tool| {
                let parameters = if use_parameters {
                    Some(sanitize_google_schema_for_open_api(&tool.parameters))
                } else {
                    None
                };
                let parameters_json_schema = if use_parameters {
                    None
                } else {
                    Some(tool.parameters.clone())
                };
                GoogleFunctionDeclaration {
                    name: tool.name.clone(),
                    description: tool.description.clone(),
                    parameters_json_schema,
                    parameters,
                }
            })
            .collect(),
    }])
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::conversation::{
        AssistantContentBlock, ImageContent, RichAssistantMessage, RichMessage, TextContent,
        ThinkingContent, ToolCall, ToolResultMessage, UserContentBlock, UserMessage,
        UserMessageContent,
    };
    use crate::types::{Model, ModelInputKind, Usage, UsageCost};
    use serde_json::json;

    #[test]
    fn detects_thinking_parts_by_thought_marker_only() {
        assert!(is_thinking_part(&GooglePartThinkingState {
            thought: true,
            thought_signature: None,
        }));
        assert!(!is_thinking_part(&GooglePartThinkingState {
            thought: false,
            thought_signature: Some("abcd".to_string()),
        }));
    }

    #[test]
    fn retains_last_non_empty_thought_signature() {
        assert_eq!(
            retain_thought_signature(Some("old"), Some("new")).as_deref(),
            Some("new")
        );
        assert_eq!(
            retain_thought_signature(Some("old"), Some("")).as_deref(),
            Some("old")
        );
    }

    #[test]
    fn validates_google_thought_signatures() {
        assert!(is_valid_google_thought_signature("YWJjZA=="));
        assert!(!is_valid_google_thought_signature("abc"));
        assert!(!is_valid_google_thought_signature("abcd-"));
    }

    #[test]
    fn keeps_signatures_only_for_same_provider_and_model() {
        assert_eq!(
            resolve_google_thought_signature(true, Some("YWJjZA==")).as_deref(),
            Some("YWJjZA==")
        );
        assert_eq!(
            resolve_google_thought_signature(false, Some("YWJjZA==")),
            None
        );
    }

    #[test]
    fn detects_models_requiring_explicit_tool_call_id() {
        assert!(requires_google_tool_call_id("claude-sonnet-4"));
        assert!(requires_google_tool_call_id("gpt-oss-120b"));
        assert!(!requires_google_tool_call_id("gemini-2.5-pro"));
    }

    #[test]
    fn parses_gemini_major_version_and_multimodal_support() {
        assert_eq!(gemini_major_version("gemini-2.5-pro"), Some(2));
        assert_eq!(gemini_major_version("gemini-live-3-preview"), Some(3));
        assert_eq!(gemini_major_version("claude-sonnet-4"), None);
        assert!(!supports_multimodal_function_response("gemini-2.5-pro"));
        assert!(supports_multimodal_function_response("gemini-3-pro"));
        assert!(supports_multimodal_function_response("claude-sonnet-4"));
    }

    #[test]
    fn maps_tool_choice_and_stop_reason() {
        assert_eq!(map_google_tool_choice("none"), GoogleToolChoiceMode::None);
        assert_eq!(
            map_google_tool_choice("unknown"),
            GoogleToolChoiceMode::Auto
        );
        assert_eq!(map_google_stop_reason("STOP"), AssistantStopReason::Stop);
        assert_eq!(
            map_google_stop_reason("MAX_TOKENS"),
            AssistantStopReason::Length
        );
        assert_eq!(map_google_stop_reason("SAFETY"), AssistantStopReason::Error);
    }

    #[test]
    fn sanitizes_schema_meta_declarations_for_open_api_parameters() {
        let schema = json!({
            "$schema": "https://json-schema.org/draft/2020-12/schema",
            "$defs": {"Ignored": {"type": "string"}},
            "type": "object",
            "properties": {
                "path": {
                    "$comment": "ignored",
                    "type": "string",
                    "items": {"$id": "item", "type": "string"}
                }
            },
            "examples": [{"$schema": "kept-inside-array"}]
        });

        let sanitized = sanitize_google_schema_for_open_api(&schema);

        assert_eq!(sanitized["type"], "object");
        assert!(sanitized.get("$schema").is_none());
        assert!(sanitized.get("$defs").is_none());
        assert!(sanitized["properties"]["path"].get("$comment").is_none());
        assert!(sanitized["properties"]["path"]["items"]
            .get("$id")
            .is_none());
        assert_eq!(sanitized["examples"][0]["$schema"], "kept-inside-array");
    }

    #[test]
    fn converts_tools_to_parameters_json_schema_by_default() {
        let tools = vec![GoogleToolDefinition {
            name: "read_file".to_string(),
            description: "读取文件".to_string(),
            parameters: json!({"type": "object"}),
        }];

        let result = convert_google_tools(&tools, false).expect("tools");

        assert_eq!(result.len(), 1);
        assert_eq!(result[0].function_declarations[0].name, "read_file");
        assert_eq!(
            result[0].function_declarations[0]
                .parameters_json_schema
                .as_ref()
                .expect("schema")["type"],
            "object"
        );
        assert!(result[0].function_declarations[0].parameters.is_none());
    }

    #[test]
    fn converts_tools_to_sanitized_open_api_parameters_when_requested() {
        let tools = vec![GoogleToolDefinition {
            name: "read_file".to_string(),
            description: "读取文件".to_string(),
            parameters: json!({"$schema": "ignored", "type": "object"}),
        }];

        let result = convert_google_tools(&tools, true).expect("tools");

        let declaration = &result[0].function_declarations[0];
        assert!(declaration.parameters_json_schema.is_none());
        let parameters = declaration.parameters.as_ref().expect("parameters");
        assert_eq!(parameters["type"], "object");
        assert!(parameters.get("$schema").is_none());
    }

    #[test]
    fn skips_empty_tools() {
        assert_eq!(convert_google_tools(&[], false), None);
    }

    #[test]
    fn converts_user_text_and_images_to_google_parts() {
        let context = GoogleMessagesContext {
            messages: vec![RichMessage::User(UserMessage {
                content: UserMessageContent::Blocks(vec![
                    UserContentBlock::Text(text("hello")),
                    UserContentBlock::Image(ImageContent {
                        data: "abc".to_string(),
                        mime_type: "image/png".to_string(),
                    }),
                ]),
                timestamp_millis: 1,
            })],
        };

        let result = convert_google_messages(&vision_model("gemini-3-pro"), &context);

        assert_eq!(result[0].role, "user");
        assert_eq!(result[0].parts[0].text.as_deref(), Some("hello"));
        assert_eq!(
            result[0].parts[1]
                .inline_data
                .as_ref()
                .map(|data| data.mime_type.as_str()),
            Some("image/png")
        );
    }

    #[test]
    fn converts_same_model_assistant_thinking_with_signature() {
        let context = GoogleMessagesContext {
            messages: vec![RichMessage::Assistant(RichAssistantMessage {
                content: vec![
                    AssistantContentBlock::Thinking(ThinkingContent {
                        thinking: "reasoning".to_string(),
                        thinking_signature: Some("YWJjZA==".to_string()),
                        redacted: false,
                    }),
                    AssistantContentBlock::Text(TextContent {
                        text: "answer".to_string(),
                        text_signature: Some("YWJjZA==".to_string()),
                    }),
                ],
                ..assistant_defaults("google", "gemini-3-pro")
            })],
        };

        let result = convert_google_messages(&vision_model("gemini-3-pro"), &context);

        assert_eq!(result[0].role, "model");
        assert_eq!(result[0].parts[0].thought, Some(true));
        assert_eq!(
            result[0].parts[0].thought_signature.as_deref(),
            Some("YWJjZA==")
        );
        assert_eq!(
            result[0].parts[1].thought_signature.as_deref(),
            Some("YWJjZA==")
        );
    }

    #[test]
    fn converts_foreign_thinking_to_plain_text_without_signature() {
        let context = GoogleMessagesContext {
            messages: vec![RichMessage::Assistant(RichAssistantMessage {
                content: vec![AssistantContentBlock::Thinking(ThinkingContent {
                    thinking: "reasoning".to_string(),
                    thinking_signature: Some("YWJjZA==".to_string()),
                    redacted: false,
                })],
                provider: "other".to_string(),
                ..assistant_defaults("google", "gemini-3-pro")
            })],
        };

        let result = convert_google_messages(&vision_model("gemini-3-pro"), &context);

        assert_eq!(result[0].parts[0].text.as_deref(), Some("reasoning"));
        assert_eq!(result[0].parts[0].thought, None);
        assert_eq!(result[0].parts[0].thought_signature, None);
    }

    #[test]
    fn includes_tool_call_id_only_for_models_that_require_it() {
        let context = GoogleMessagesContext {
            messages: vec![RichMessage::Assistant(RichAssistantMessage {
                content: vec![AssistantContentBlock::ToolCall(ToolCall {
                    id: "call id!".to_string(),
                    name: "read_file".to_string(),
                    arguments: std::collections::BTreeMap::from([(
                        "path".to_string(),
                        json!("README.md"),
                    )]),
                    thought_signature: None,
                })],
                provider: "other".to_string(),
                ..assistant_defaults("google", "claude-sonnet-4")
            })],
        };

        let claude_result = convert_google_messages(&text_model("claude-sonnet-4"), &context);
        let gemini_result = convert_google_messages(&text_model("gemini-3-pro"), &context);

        assert_eq!(
            claude_result[0].parts[0]
                .function_call
                .as_ref()
                .and_then(|call| call.id.as_deref()),
            Some("call_id_")
        );
        assert_eq!(
            gemini_result[0].parts[0]
                .function_call
                .as_ref()
                .and_then(|call| call.id.as_deref()),
            None
        );
    }

    #[test]
    fn merges_adjacent_tool_results_into_one_user_turn() {
        let context = GoogleMessagesContext {
            messages: vec![
                tool_result("call_1", "read_file", "ok", false),
                tool_result("call_2", "list_files", "done", true),
            ],
        };

        let result = convert_google_messages(&text_model("claude-sonnet-4"), &context);

        assert_eq!(result.len(), 1);
        assert_eq!(result[0].role, "user");
        assert_eq!(result[0].parts.len(), 2);
        assert_eq!(
            result[0].parts[0]
                .function_response
                .as_ref()
                .and_then(|response| response.response.get("output")),
            Some(&json!("ok"))
        );
        assert_eq!(
            result[0].parts[1]
                .function_response
                .as_ref()
                .and_then(|response| response.response.get("error")),
            Some(&json!("done"))
        );
    }

    #[test]
    fn emits_separate_image_turn_for_gemini_before_three() {
        let context = GoogleMessagesContext {
            messages: vec![RichMessage::ToolResult(ToolResultMessage {
                tool_call_id: "call_1".to_string(),
                tool_name: "screenshot".to_string(),
                content: vec![
                    UserContentBlock::Text(text("see image")),
                    UserContentBlock::Image(ImageContent {
                        data: "abc".to_string(),
                        mime_type: "image/png".to_string(),
                    }),
                ],
                details: None,
                is_error: false,
                timestamp_millis: 1,
            })],
        };

        let result = convert_google_messages(&vision_model("gemini-2.5-pro"), &context);

        assert_eq!(result.len(), 2);
        assert!(result[0].parts[0]
            .function_response
            .as_ref()
            .expect("function response")
            .parts
            .is_none());
        assert_eq!(
            result[1].parts[0].text.as_deref(),
            Some("Tool result image:")
        );
        assert!(result[1].parts[1].inline_data.is_some());
    }

    #[test]
    fn nests_tool_result_images_for_gemini_three() {
        let context = GoogleMessagesContext {
            messages: vec![RichMessage::ToolResult(ToolResultMessage {
                tool_call_id: "call_1".to_string(),
                tool_name: "screenshot".to_string(),
                content: vec![UserContentBlock::Image(ImageContent {
                    data: "abc".to_string(),
                    mime_type: "image/png".to_string(),
                })],
                details: None,
                is_error: false,
                timestamp_millis: 1,
            })],
        };

        let result = convert_google_messages(&vision_model("gemini-3-pro"), &context);

        assert_eq!(result.len(), 1);
        assert_eq!(
            result[0].parts[0]
                .function_response
                .as_ref()
                .and_then(|response| response.parts.as_ref())
                .map(Vec::len),
            Some(1)
        );
    }

    fn text(value: &str) -> TextContent {
        TextContent {
            text: value.to_string(),
            text_signature: None,
        }
    }

    fn assistant_defaults(provider: &str, model: &str) -> RichAssistantMessage {
        RichAssistantMessage {
            content: Vec::new(),
            api: "google-generative-ai".to_string(),
            provider: provider.to_string(),
            model: model.to_string(),
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

    fn tool_result(id: &str, name: &str, output: &str, is_error: bool) -> RichMessage {
        RichMessage::ToolResult(ToolResultMessage {
            tool_call_id: id.to_string(),
            tool_name: name.to_string(),
            content: vec![UserContentBlock::Text(text(output))],
            details: None,
            is_error,
            timestamp_millis: 1,
        })
    }

    fn text_model(id: &str) -> Model {
        Model {
            id: id.to_string(),
            provider: "google".to_string(),
            api: "google-generative-ai".to_string(),
            display_name: id.to_string(),
            context_window: 1,
            input: vec![ModelInputKind::Text],
            ..Model::default()
        }
    }

    fn vision_model(id: &str) -> Model {
        Model {
            input: vec![ModelInputKind::Text, ModelInputKind::Image],
            ..text_model(id)
        }
    }
}
