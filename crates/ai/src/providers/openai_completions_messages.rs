use serde_json::Value;

use crate::conversation::{
    transform_messages, AssistantContentBlock, RichAssistantMessage, RichMessage, TextContent,
    UserContentBlock, UserMessageContent,
};
use crate::providers::openai_completions_types::{
    resolve_openai_completions_cache_control, OpenAiCompatCacheControl, OpenAiCompletionsCompat,
    OpenAiCompletionsContentPart, OpenAiCompletionsContext, OpenAiCompletionsFunctionCall,
    OpenAiCompletionsMessage, OpenAiCompletionsMessageContent, OpenAiCompletionsThinkingFormat,
    OpenAiCompletionsToolCall, OpenAiImageUrl,
};
use crate::types::{Model, ModelInputKind};
use crate::utils::sanitize_surrogates;

pub fn convert_openai_completions_messages(
    model: &Model,
    context: &OpenAiCompletionsContext,
    compat: &OpenAiCompletionsCompat,
) -> Vec<OpenAiCompletionsMessage> {
    convert_openai_completions_messages_with_cache_retention(model, context, compat, None)
}

pub fn convert_openai_completions_messages_with_cache_retention(
    model: &Model,
    context: &OpenAiCompletionsContext,
    compat: &OpenAiCompletionsCompat,
    cache_retention: Option<&str>,
) -> Vec<OpenAiCompletionsMessage> {
    let transformed_messages = transform_messages(
        &context.messages,
        model,
        Some(normalize_openai_completions_tool_call_id_for_model),
    );
    let mut messages = Vec::new();

    if let Some(system_prompt) = context
        .system_prompt
        .as_deref()
        .filter(|prompt| !prompt.is_empty())
    {
        messages.push(OpenAiCompletionsMessage {
            role: if model.reasoning.is_some() && compat.supports_developer_role {
                "developer"
            } else {
                "system"
            }
            .to_string(),
            content: Some(OpenAiCompletionsMessageContent::Text(sanitize_surrogates(
                system_prompt,
            ))),
            tool_calls: None,
            tool_call_id: None,
            name: None,
            extra: serde_json::Map::new(),
        });
    }

    let mut last_role: Option<&'static str> = None;
    let mut index = 0;
    while index < transformed_messages.len() {
        match transformed_messages[index].clone() {
            RichMessage::User(user) => {
                if compat.requires_assistant_after_tool_result && last_role == Some("toolResult") {
                    messages.push(openai_assistant_text_message(
                        "I have processed the tool results.",
                    ));
                }
                if let Some(content) = openai_user_content(user.content) {
                    messages.push(OpenAiCompletionsMessage {
                        role: "user".to_string(),
                        content: Some(content),
                        tool_calls: None,
                        tool_call_id: None,
                        name: None,
                        extra: serde_json::Map::new(),
                    });
                }
                last_role = Some("user");
            }
            RichMessage::Assistant(assistant) => {
                let mut message = OpenAiCompletionsMessage {
                    role: "assistant".to_string(),
                    content: None,
                    tool_calls: None,
                    tool_call_id: None,
                    name: None,
                    extra: serde_json::Map::new(),
                };
                let text_parts = assistant
                    .content
                    .iter()
                    .filter_map(|block| match block {
                        AssistantContentBlock::Text(text) if !text.text.trim().is_empty() => {
                            Some(sanitize_surrogates(&text.text))
                        }
                        _ => None,
                    })
                    .collect::<Vec<_>>();
                let thinking_parts = assistant
                    .content
                    .iter()
                    .filter_map(|block| match block {
                        AssistantContentBlock::Thinking(thinking)
                            if !thinking.thinking.trim().is_empty() =>
                        {
                            Some(thinking)
                        }
                        _ => None,
                    })
                    .collect::<Vec<_>>();
                if !thinking_parts.is_empty() {
                    if compat.requires_thinking_as_text {
                        let parts = assistant
                            .content
                            .iter()
                            .filter_map(|block| match block {
                                AssistantContentBlock::Thinking(thinking)
                                    if !thinking.thinking.trim().is_empty() =>
                                {
                                    Some(OpenAiCompletionsContentPart::Text {
                                        text: sanitize_surrogates(&thinking.thinking),
                                        cache_control: None,
                                    })
                                }
                                AssistantContentBlock::Text(text)
                                    if !text.text.trim().is_empty() =>
                                {
                                    Some(OpenAiCompletionsContentPart::Text {
                                        text: sanitize_surrogates(&text.text),
                                        cache_control: None,
                                    })
                                }
                                _ => None,
                            })
                            .collect::<Vec<_>>();
                        if !parts.is_empty() {
                            message.content = Some(OpenAiCompletionsMessageContent::Parts(parts));
                        }
                    } else if compat.thinking_format == OpenAiCompletionsThinkingFormat::OpenAi {
                        if !text_parts.is_empty() {
                            message.content =
                                Some(OpenAiCompletionsMessageContent::Text(text_parts.join("")));
                        }
                        if let Some(mut signature) = thinking_parts
                            .first()
                            .and_then(|thinking| thinking.thinking_signature.as_deref())
                            .filter(|signature| !signature.is_empty())
                        {
                            if model.provider == "opencode-go" && signature == "reasoning" {
                                signature = "reasoning_content";
                            }
                            message.extra.insert(
                                signature.to_string(),
                                Value::String(
                                    thinking_parts
                                        .iter()
                                        .map(|thinking| thinking.thinking.as_str())
                                        .collect::<Vec<_>>()
                                        .join("\n"),
                                ),
                            );
                        }
                    } else {
                        let thinking_text = thinking_parts
                            .iter()
                            .map(|thinking| sanitize_surrogates(&thinking.thinking))
                            .collect::<Vec<_>>()
                            .join("\n\n");
                        let mut content = vec![thinking_text];
                        content.extend(text_parts);
                        message.content =
                            Some(OpenAiCompletionsMessageContent::Text(content.join("")));
                    }
                } else if !text_parts.is_empty() {
                    message.content =
                        Some(OpenAiCompletionsMessageContent::Text(text_parts.join("")));
                }

                let tool_calls = assistant
                    .content
                    .iter()
                    .filter_map(|block| match block {
                        AssistantContentBlock::ToolCall(tool_call) => {
                            Some(OpenAiCompletionsToolCall {
                                id: tool_call.id.clone(),
                                r#type: "function".to_string(),
                                function: OpenAiCompletionsFunctionCall {
                                    name: tool_call.name.clone(),
                                    arguments: serde_json::to_string(&tool_call.arguments)
                                        .unwrap_or_else(|_| "{}".to_string()),
                                },
                            })
                        }
                        _ => None,
                    })
                    .collect::<Vec<_>>();
                if !tool_calls.is_empty() {
                    message.tool_calls = Some(tool_calls);
                }
                if compat.requires_reasoning_content_on_assistant_messages
                    && model.reasoning.is_some()
                    && !message.extra.contains_key("reasoning_content")
                {
                    message.extra.insert(
                        "reasoning_content".to_string(),
                        Value::String(String::new()),
                    );
                }
                if message.content.is_some()
                    || message.tool_calls.is_some()
                    || !message.extra.is_empty()
                {
                    messages.push(message);
                }
                last_role = Some("assistant");
            }
            RichMessage::ToolResult(_) => {
                let mut image_parts = Vec::new();
                let mut next_index = index;
                while next_index < transformed_messages.len() {
                    let RichMessage::ToolResult(tool_result) =
                        transformed_messages[next_index].clone()
                    else {
                        break;
                    };
                    let text_result = tool_result
                        .content
                        .iter()
                        .filter_map(|block| match block {
                            UserContentBlock::Text(TextContent { text, .. }) => Some(text.as_str()),
                            UserContentBlock::Image(_) => None,
                        })
                        .collect::<Vec<_>>()
                        .join("\n");
                    let has_text = !text_result.is_empty();
                    let has_images = tool_result
                        .content
                        .iter()
                        .any(|block| matches!(block, UserContentBlock::Image(_)));
                    messages.push(OpenAiCompletionsMessage {
                        role: "tool".to_string(),
                        content: Some(OpenAiCompletionsMessageContent::Text(sanitize_surrogates(
                            if has_text {
                                &text_result
                            } else {
                                "(see attached image)"
                            },
                        ))),
                        tool_calls: None,
                        tool_call_id: Some(tool_result.tool_call_id),
                        name: (compat.requires_tool_result_name
                            && !tool_result.tool_name.is_empty())
                        .then_some(tool_result.tool_name),
                        extra: serde_json::Map::new(),
                    });
                    if has_images && model_supports_images(model) {
                        for block in tool_result.content {
                            if let UserContentBlock::Image(image) = block {
                                image_parts.push(OpenAiCompletionsContentPart::ImageUrl {
                                    image_url: OpenAiImageUrl {
                                        url: format!(
                                            "data:{};base64,{}",
                                            image.mime_type, image.data
                                        ),
                                    },
                                });
                            }
                        }
                    }
                    next_index += 1;
                }
                if !image_parts.is_empty() {
                    if compat.requires_assistant_after_tool_result {
                        messages.push(openai_assistant_text_message(
                            "I have processed the tool results.",
                        ));
                    }
                    let mut content = vec![OpenAiCompletionsContentPart::Text {
                        text: "Attached image(s) from tool result:".to_string(),
                        cache_control: None,
                    }];
                    content.extend(image_parts);
                    messages.push(OpenAiCompletionsMessage {
                        role: "user".to_string(),
                        content: Some(OpenAiCompletionsMessageContent::Parts(content)),
                        tool_calls: None,
                        tool_call_id: None,
                        name: None,
                        extra: serde_json::Map::new(),
                    });
                    last_role = Some("user");
                } else {
                    last_role = Some("toolResult");
                }
                index = next_index - 1;
            }
        }
        index += 1;
    }

    if let Some(cache_control) = resolve_openai_completions_cache_control(compat, cache_retention) {
        apply_anthropic_cache_control(&mut messages, cache_control);
    }

    messages
}

fn openai_user_content(content: UserMessageContent) -> Option<OpenAiCompletionsMessageContent> {
    match content {
        UserMessageContent::Text(text) => Some(OpenAiCompletionsMessageContent::Text(
            sanitize_surrogates(&text),
        )),
        UserMessageContent::Blocks(blocks) => {
            let parts = blocks
                .into_iter()
                .map(|block| match block {
                    UserContentBlock::Text(text) => OpenAiCompletionsContentPart::Text {
                        text: sanitize_surrogates(&text.text),
                        cache_control: None,
                    },
                    UserContentBlock::Image(image) => OpenAiCompletionsContentPart::ImageUrl {
                        image_url: OpenAiImageUrl {
                            url: format!("data:{};base64,{}", image.mime_type, image.data),
                        },
                    },
                })
                .collect::<Vec<_>>();
            (!parts.is_empty()).then_some(OpenAiCompletionsMessageContent::Parts(parts))
        }
    }
}

fn openai_assistant_text_message(text: &str) -> OpenAiCompletionsMessage {
    OpenAiCompletionsMessage {
        role: "assistant".to_string(),
        content: Some(OpenAiCompletionsMessageContent::Text(text.to_string())),
        tool_calls: None,
        tool_call_id: None,
        name: None,
        extra: serde_json::Map::new(),
    }
}

fn apply_anthropic_cache_control(
    messages: &mut [OpenAiCompletionsMessage],
    cache_control: OpenAiCompatCacheControl,
) {
    if let Some(message) = messages
        .iter_mut()
        .find(|message| message.role == "system" || message.role == "developer")
    {
        add_cache_control_to_message_text(message, cache_control.clone());
    }

    for message in messages.iter_mut().rev() {
        if (message.role == "user" || message.role == "assistant")
            && add_cache_control_to_message_text(message, cache_control.clone())
        {
            break;
        }
    }
}

fn add_cache_control_to_message_text(
    message: &mut OpenAiCompletionsMessage,
    cache_control: OpenAiCompatCacheControl,
) -> bool {
    match message.content.take() {
        Some(OpenAiCompletionsMessageContent::Text(text)) if !text.is_empty() => {
            message.content = Some(OpenAiCompletionsMessageContent::Parts(vec![
                OpenAiCompletionsContentPart::Text {
                    text,
                    cache_control: Some(cache_control),
                },
            ]));
            true
        }
        Some(OpenAiCompletionsMessageContent::Parts(mut parts)) => {
            for part in parts.iter_mut().rev() {
                if let OpenAiCompletionsContentPart::Text {
                    cache_control: existing,
                    ..
                } = part
                {
                    *existing = Some(cache_control);
                    message.content = Some(OpenAiCompletionsMessageContent::Parts(parts));
                    return true;
                }
            }
            message.content = Some(OpenAiCompletionsMessageContent::Parts(parts));
            false
        }
        content => {
            message.content = content;
            false
        }
    }
}

fn normalize_openai_completions_tool_call_id_for_model(
    id: &str,
    target_model: &Model,
    _source: &RichAssistantMessage,
) -> String {
    normalize_openai_completions_tool_call_id(id, &target_model.provider)
}

fn normalize_openai_completions_tool_call_id(id: &str, provider: &str) -> String {
    let id = id.split('|').next().unwrap_or(id);
    if provider == "openai" {
        id.chars().take(40).collect()
    } else {
        id.to_string()
    }
}

fn model_supports_images(model: &Model) -> bool {
    model
        .input
        .iter()
        .any(|kind| matches!(kind, ModelInputKind::Image))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::conversation::{
        ImageContent, RichAssistantMessage, ToolCall, ToolResultMessage, UserMessage,
    };
    use crate::providers::openai::detect_openai_completions_compat;
    use crate::types::{AssistantStopReason, Usage, UsageCost};
    use serde_json::json;

    #[test]
    fn converts_rich_messages_to_chat_completions_messages() {
        let model = vision_model("gpt-4o");
        let compat = detect_openai_completions_compat("openai", "https://api.openai.com/v1");
        let context = OpenAiCompletionsContext {
            system_prompt: Some("system".to_string()),
            messages: vec![
                RichMessage::User(UserMessage {
                    content: UserMessageContent::Blocks(vec![
                        UserContentBlock::Text(text("hello")),
                        UserContentBlock::Image(ImageContent {
                            data: "abc".to_string(),
                            mime_type: "image/png".to_string(),
                        }),
                    ]),
                    timestamp_millis: 1,
                }),
                RichMessage::Assistant(RichAssistantMessage {
                    content: vec![AssistantContentBlock::ToolCall(ToolCall {
                        id: "call.with.invalid/chars/and/very/long/value/that/should/truncate"
                            .to_string(),
                        name: "read_file".to_string(),
                        arguments: serde_json::Map::from_iter([(
                            "path".to_string(),
                            json!("/tmp/a"),
                        )])
                        .into_iter()
                        .collect(),
                        thought_signature: None,
                    })],
                    provider: "other".to_string(),
                    ..assistant_defaults()
                }),
                RichMessage::ToolResult(ToolResultMessage {
                    tool_call_id:
                        "call.with.invalid/chars/and/very/long/value/that/should/truncate"
                            .to_string(),
                    tool_name: "read_file".to_string(),
                    content: vec![
                        UserContentBlock::Text(text("done")),
                        UserContentBlock::Image(ImageContent {
                            data: "img".to_string(),
                            mime_type: "image/jpeg".to_string(),
                        }),
                    ],
                    details: None,
                    is_error: false,
                    timestamp_millis: 1,
                }),
            ],
        };

        let messages = convert_openai_completions_messages(&model, &context, &compat);

        assert_eq!(messages[0].role, "system");
        assert_eq!(messages[1].role, "user");
        assert!(matches!(
            messages[1].content,
            Some(OpenAiCompletionsMessageContent::Parts(_))
        ));
        let tool_call = messages[2]
            .tool_calls
            .as_ref()
            .and_then(|tool_calls| tool_calls.first())
            .expect("tool call");
        assert_eq!(tool_call.id.chars().count(), 40);
        assert_eq!(tool_call.function.name, "read_file");
        assert_eq!(messages[3].role, "tool");
        assert_eq!(
            messages[3].tool_call_id.as_deref(),
            Some(tool_call.id.as_str())
        );
        assert_eq!(messages[4].role, "user");
        assert!(matches!(
            messages[4].content,
            Some(OpenAiCompletionsMessageContent::Parts(_))
        ));
    }

    #[test]
    fn converts_thinking_to_provider_specific_field() {
        let model = model("gpt-oss");
        let compat = detect_openai_completions_compat("openai", "https://api.openai.com/v1");
        let messages = convert_openai_completions_messages(
            &model,
            &OpenAiCompletionsContext {
                system_prompt: None,
                messages: vec![RichMessage::Assistant(RichAssistantMessage {
                    content: vec![AssistantContentBlock::Thinking(
                        crate::conversation::ThinkingContent {
                            thinking: "reason".to_string(),
                            thinking_signature: Some("reasoning_content".to_string()),
                            redacted: false,
                        },
                    )],
                    model: "gpt-oss".to_string(),
                    ..assistant_defaults()
                })],
            },
            &compat,
        );

        assert_eq!(
            messages[0].extra.get("reasoning_content"),
            Some(&Value::String("reason".to_string()))
        );
    }

    #[test]
    fn replays_empty_reasoning_content_for_required_reasoning_assistant_messages_like_pi() {
        let mut model = model("mimo-v2.5-pro");
        model.provider = "xiaomi".to_string();
        model.reasoning = Some(crate::types::ModelReasoning { enabled: true });
        let mut compat =
            detect_openai_completions_compat("xiaomi", "https://api.xiaomimimo.com/v1");
        compat.requires_reasoning_content_on_assistant_messages = true;

        let messages = convert_openai_completions_messages(
            &model,
            &OpenAiCompletionsContext {
                system_prompt: None,
                messages: vec![RichMessage::Assistant(RichAssistantMessage {
                    content: vec![AssistantContentBlock::ToolCall(ToolCall {
                        id: "call_1".to_string(),
                        name: "read".to_string(),
                        arguments: serde_json::Map::from_iter([(
                            "path".to_string(),
                            json!("README.md"),
                        )])
                        .into_iter()
                        .collect(),
                        thought_signature: None,
                    })],
                    provider: "xiaomi".to_string(),
                    model: "mimo-v2.5-pro".to_string(),
                    ..assistant_defaults()
                })],
            },
            &compat,
        );

        assert_eq!(
            messages[0].extra.get("reasoning_content"),
            Some(&Value::String(String::new()))
        );
    }

    #[test]
    fn replays_opencode_go_reasoning_blocks_as_reasoning_content_like_pi() {
        let mut model = model("kimi-k2.6");
        model.provider = "opencode-go".to_string();
        let compat = detect_openai_completions_compat("opencode-go", "https://api.opencode.ai/v1");

        let messages = convert_openai_completions_messages(
            &model,
            &OpenAiCompletionsContext {
                system_prompt: None,
                messages: vec![RichMessage::Assistant(RichAssistantMessage {
                    content: vec![
                        AssistantContentBlock::Thinking(crate::conversation::ThinkingContent {
                            thinking: "think".to_string(),
                            thinking_signature: Some("reasoning".to_string()),
                            redacted: false,
                        }),
                        AssistantContentBlock::ToolCall(ToolCall {
                            id: "call_1".to_string(),
                            name: "read".to_string(),
                            arguments: serde_json::Map::from_iter([(
                                "path".to_string(),
                                json!("README.md"),
                            )])
                            .into_iter()
                            .collect(),
                            thought_signature: None,
                        }),
                    ],
                    provider: "opencode-go".to_string(),
                    model: "kimi-k2.6".to_string(),
                    ..assistant_defaults()
                })],
            },
            &compat,
        );

        assert_eq!(
            messages[0].extra.get("reasoning_content"),
            Some(&Value::String("think".to_string()))
        );
        assert!(!messages[0].extra.contains_key("reasoning"));
    }

    #[test]
    fn replays_thinking_as_text_parts_when_required_like_pi() {
        let mut model = model("repro-model");
        model.provider = "repro-provider".to_string();
        model.reasoning = Some(crate::types::ModelReasoning { enabled: true });
        let mut compat =
            detect_openai_completions_compat("repro-provider", "https://example.com/v1");
        compat.requires_thinking_as_text = true;

        let messages = convert_openai_completions_messages(
            &model,
            &OpenAiCompletionsContext {
                system_prompt: None,
                messages: vec![RichMessage::Assistant(RichAssistantMessage {
                    content: vec![
                        AssistantContentBlock::Thinking(crate::conversation::ThinkingContent {
                            thinking: "internal reasoning".to_string(),
                            thinking_signature: None,
                            redacted: false,
                        }),
                        AssistantContentBlock::Text(text("visible answer")),
                    ],
                    provider: "repro-provider".to_string(),
                    model: "repro-model".to_string(),
                    ..assistant_defaults()
                })],
            },
            &compat,
        );

        let value = serde_json::to_value(messages).expect("messages serialize");
        assert_eq!(
            value[0]["content"],
            json!([
                { "type": "text", "text": "internal reasoning" },
                { "type": "text", "text": "visible answer" }
            ])
        );
    }

    #[test]
    fn replays_thinking_only_as_text_part_when_required_like_pi() {
        let mut model = model("repro-model");
        model.provider = "repro-provider".to_string();
        model.reasoning = Some(crate::types::ModelReasoning { enabled: true });
        let mut compat =
            detect_openai_completions_compat("repro-provider", "https://example.com/v1");
        compat.requires_thinking_as_text = true;

        let messages = convert_openai_completions_messages(
            &model,
            &OpenAiCompletionsContext {
                system_prompt: None,
                messages: vec![RichMessage::Assistant(RichAssistantMessage {
                    content: vec![AssistantContentBlock::Thinking(
                        crate::conversation::ThinkingContent {
                            thinking: "internal reasoning".to_string(),
                            thinking_signature: None,
                            redacted: false,
                        },
                    )],
                    provider: "repro-provider".to_string(),
                    model: "repro-model".to_string(),
                    ..assistant_defaults()
                })],
            },
            &compat,
        );

        let value = serde_json::to_value(messages).expect("messages serialize");
        assert_eq!(
            value[0]["content"],
            json!([{ "type": "text", "text": "internal reasoning" }])
        );
    }

    #[test]
    fn omits_tool_result_name_unless_required_like_pi() {
        let model = model("gpt-4o");
        let mut compat = detect_openai_completions_compat("openai", "https://api.openai.com/v1");
        compat.requires_tool_result_name = false;

        let messages = convert_openai_completions_messages(
            &model,
            &OpenAiCompletionsContext {
                system_prompt: None,
                messages: vec![RichMessage::ToolResult(ToolResultMessage {
                    tool_call_id: "call_1".to_string(),
                    tool_name: "read".to_string(),
                    content: vec![UserContentBlock::Text(text("done"))],
                    details: None,
                    is_error: false,
                    timestamp_millis: 1,
                })],
            },
            &compat,
        );

        assert_eq!(messages[0].role, "tool");
        assert!(messages[0].name.is_none());
    }

    #[test]
    fn inserts_assistant_bridge_before_tool_result_images_when_required_like_pi() {
        let model = vision_model("gpt-4o");
        let mut compat = detect_openai_completions_compat("openai", "https://api.openai.com/v1");
        compat.requires_assistant_after_tool_result = true;

        let messages = convert_openai_completions_messages(
            &model,
            &OpenAiCompletionsContext {
                system_prompt: None,
                messages: vec![RichMessage::ToolResult(ToolResultMessage {
                    tool_call_id: "call_1".to_string(),
                    tool_name: "read".to_string(),
                    content: vec![
                        UserContentBlock::Text(text("Read image file [image/png]")),
                        UserContentBlock::Image(ImageContent {
                            data: "ZmFrZQ==".to_string(),
                            mime_type: "image/png".to_string(),
                        }),
                    ],
                    details: None,
                    is_error: false,
                    timestamp_millis: 1,
                })],
            },
            &compat,
        );

        assert_eq!(
            messages
                .iter()
                .map(|message| message.role.as_str())
                .collect::<Vec<_>>(),
            vec!["tool", "assistant", "user"]
        );
        assert_eq!(
            messages[1].content,
            Some(OpenAiCompletionsMessageContent::Text(
                "I have processed the tool results.".to_string()
            ))
        );
    }

    #[test]
    fn applies_anthropic_cache_control_to_instruction_and_last_message_like_pi() {
        let model = model("custom-qwen");
        let mut compat = detect_openai_completions_compat("openrouter", "https://example.com/v1");
        compat.cache_control_format = Some("anthropic".to_string());
        let context = OpenAiCompletionsContext {
            system_prompt: Some("System prompt".to_string()),
            messages: vec![RichMessage::User(UserMessage {
                content: UserMessageContent::Text("Hello".to_string()),
                timestamp_millis: 1,
            })],
        };

        let messages = convert_openai_completions_messages(&model, &context, &compat);
        let value = serde_json::to_value(messages).expect("messages serialize");

        assert_eq!(
            value[0]["content"][0]["cache_control"],
            json!({ "type": "ephemeral" })
        );
        assert_eq!(
            value[1]["content"][0]["cache_control"],
            json!({ "type": "ephemeral" })
        );
    }

    #[test]
    fn applies_anthropic_cache_control_retention_rules_like_pi() {
        let model = model("custom-qwen");
        let mut compat = detect_openai_completions_compat("openrouter", "https://example.com/v1");
        compat.cache_control_format = Some("anthropic".to_string());
        let context = OpenAiCompletionsContext {
            system_prompt: Some("System prompt".to_string()),
            messages: vec![RichMessage::User(UserMessage {
                content: UserMessageContent::Text("Hello".to_string()),
                timestamp_millis: 1,
            })],
        };

        let none_messages = convert_openai_completions_messages_with_cache_retention(
            &model,
            &context,
            &compat,
            Some("none"),
        );
        let none_value = serde_json::to_value(none_messages).expect("messages serialize");
        assert!(none_value[0]["content"].is_string());
        assert!(none_value[1]["content"].is_string());

        let long_messages = convert_openai_completions_messages_with_cache_retention(
            &model,
            &context,
            &compat,
            Some("long"),
        );
        let long_value = serde_json::to_value(long_messages).expect("messages serialize");
        assert_eq!(
            long_value[0]["content"][0]["cache_control"],
            json!({ "type": "ephemeral", "ttl": "1h" })
        );
        assert_eq!(
            long_value[1]["content"][0]["cache_control"],
            json!({ "type": "ephemeral", "ttl": "1h" })
        );
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
            api: "openai-completions".to_string(),
            provider: "openai".to_string(),
            model: "gpt-4o".to_string(),
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

    fn model(id: &str) -> Model {
        Model {
            id: id.to_string(),
            provider: "openai".to_string(),
            api: "openai-completions".to_string(),
            display_name: id.to_string(),
            context_window: 128_000,
            ..Model::default()
        }
    }

    fn vision_model(id: &str) -> Model {
        Model {
            input: vec![ModelInputKind::Text, ModelInputKind::Image],
            ..model(id)
        }
    }
}
