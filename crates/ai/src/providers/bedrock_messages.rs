use crate::conversation::{
    transform_messages, AssistantContentBlock, RichMessage, TextContent, UserContentBlock,
    UserMessageContent,
};
use crate::providers::bedrock::{
    bedrock_supports_prompt_caching, bedrock_supports_thinking_signature,
};
use crate::providers::bedrock_types::{
    BedrockCachePoint, BedrockCacheRetention, BedrockContentBlock, BedrockConversationRole,
    BedrockImageBlock, BedrockImageFormat, BedrockImageSource, BedrockMessage,
    BedrockMessagesContext, BedrockReasoningContent, BedrockReasoningText,
    BedrockSystemContentBlock, BedrockToolChoice, BedrockToolConfiguration, BedrockToolDefinition,
    BedrockToolResult, BedrockToolResultStatus, BedrockToolSpec, BedrockToolSpecBody,
    BedrockToolUse,
};
use crate::types::Model;
use serde_json::json;

pub fn build_bedrock_system_prompt(
    system_prompt: Option<&str>,
    model: &Model,
    cache_retention: BedrockCacheRetention,
    force_cache: bool,
) -> Option<Vec<BedrockSystemContentBlock>> {
    let system_prompt = system_prompt.filter(|prompt| !prompt.is_empty())?;
    let mut blocks = vec![BedrockSystemContentBlock {
        text: Some(crate::utils::sanitize_surrogates(system_prompt)),
        cache_point: None,
    }];
    if cache_retention != BedrockCacheRetention::None
        && bedrock_supports_prompt_caching(model, force_cache)
    {
        blocks.push(BedrockSystemContentBlock {
            text: None,
            cache_point: Some(bedrock_cache_point(cache_retention)),
        });
    }
    Some(blocks)
}

pub fn normalize_bedrock_tool_call_id(id: &str) -> String {
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

pub fn convert_bedrock_tool_config(
    tools: &[BedrockToolDefinition],
    tool_choice: Option<&BedrockToolChoice>,
) -> Option<BedrockToolConfiguration> {
    if tools.is_empty() || matches!(tool_choice, Some(BedrockToolChoice::None)) {
        return None;
    }
    let tools = tools
        .iter()
        .map(|tool| BedrockToolSpec {
            tool_spec: BedrockToolSpecBody {
                name: tool.name.clone(),
                description: tool.description.clone(),
                input_schema: json!({ "json": tool.parameters }),
            },
        })
        .collect::<Vec<_>>();
    let tool_choice = match tool_choice {
        Some(BedrockToolChoice::Auto) => Some(json!({ "auto": {} })),
        Some(BedrockToolChoice::Any) => Some(json!({ "any": {} })),
        Some(BedrockToolChoice::Tool { name }) => Some(json!({ "tool": { "name": name } })),
        Some(BedrockToolChoice::None) | None => None,
    };
    Some(BedrockToolConfiguration { tools, tool_choice })
}

pub fn convert_bedrock_messages(
    model: &Model,
    context: &BedrockMessagesContext,
    cache_retention: BedrockCacheRetention,
    force_cache: bool,
) -> Result<Vec<BedrockMessage>, String> {
    let transformed_messages = transform_messages(
        &context.messages,
        model,
        Some(normalize_bedrock_tool_call_id_for_model),
    );
    let mut result = Vec::new();
    let mut index = 0;

    while index < transformed_messages.len() {
        match transformed_messages[index].clone() {
            RichMessage::User(user) => {
                if let Some(content) = bedrock_blocks_from_user_content(user.content)? {
                    result.push(BedrockMessage {
                        role: BedrockConversationRole::User,
                        content,
                    });
                }
            }
            RichMessage::Assistant(assistant) => {
                if assistant.content.is_empty() {
                    index += 1;
                    continue;
                }
                let mut content = Vec::new();
                for block in assistant.content {
                    match block {
                        AssistantContentBlock::Text(text) => {
                            if text.text.trim().is_empty() {
                                continue;
                            }
                            content.push(bedrock_text_block(&text.text));
                        }
                        AssistantContentBlock::ToolCall(tool_call) => {
                            content.push(BedrockContentBlock {
                                text: None,
                                image: None,
                                tool_use: Some(BedrockToolUse {
                                    tool_use_id: tool_call.id,
                                    name: tool_call.name,
                                    input: tool_call.arguments,
                                }),
                                tool_result: None,
                                reasoning_content: None,
                                cache_point: None,
                            });
                        }
                        AssistantContentBlock::Thinking(thinking) => {
                            if thinking.thinking.trim().is_empty() {
                                continue;
                            }
                            if bedrock_supports_thinking_signature(model) {
                                match thinking.thinking_signature {
                                    Some(signature) if !signature.trim().is_empty() => {
                                        content.push(bedrock_reasoning_block(
                                            &thinking.thinking,
                                            Some(signature),
                                        ));
                                    }
                                    _ => content.push(bedrock_text_block(&thinking.thinking)),
                                }
                            } else {
                                content.push(bedrock_reasoning_block(&thinking.thinking, None));
                            }
                        }
                    }
                }
                if !content.is_empty() {
                    result.push(BedrockMessage {
                        role: BedrockConversationRole::Assistant,
                        content,
                    });
                }
            }
            RichMessage::ToolResult(tool_result) => {
                let mut content = vec![bedrock_tool_result_block(tool_result)?];
                let mut next_index = index + 1;
                while next_index < transformed_messages.len() {
                    let RichMessage::ToolResult(next_tool_result) =
                        transformed_messages[next_index].clone()
                    else {
                        break;
                    };
                    content.push(bedrock_tool_result_block(next_tool_result)?);
                    next_index += 1;
                }
                result.push(BedrockMessage {
                    role: BedrockConversationRole::User,
                    content,
                });
                index = next_index - 1;
            }
        }
        index += 1;
    }

    if cache_retention != BedrockCacheRetention::None
        && bedrock_supports_prompt_caching(model, force_cache)
    {
        if let Some(last_message) = result.last_mut() {
            if last_message.role == BedrockConversationRole::User {
                last_message.content.push(BedrockContentBlock {
                    text: None,
                    image: None,
                    tool_use: None,
                    tool_result: None,
                    reasoning_content: None,
                    cache_point: Some(bedrock_cache_point(cache_retention)),
                });
            }
        }
    }

    Ok(result)
}

pub fn bedrock_image_format(mime_type: &str) -> Option<BedrockImageFormat> {
    match mime_type {
        "image/jpeg" | "image/jpg" => Some(BedrockImageFormat::Jpeg),
        "image/png" => Some(BedrockImageFormat::Png),
        "image/gif" => Some(BedrockImageFormat::Gif),
        "image/webp" => Some(BedrockImageFormat::Webp),
        _ => None,
    }
}

fn bedrock_cache_point(cache_retention: BedrockCacheRetention) -> BedrockCachePoint {
    BedrockCachePoint {
        r#type: "default".to_string(),
        ttl: (cache_retention == BedrockCacheRetention::Long).then(|| "ONE_HOUR".to_string()),
    }
}

fn bedrock_blocks_from_user_content(
    content: UserMessageContent,
) -> Result<Option<Vec<BedrockContentBlock>>, String> {
    match content {
        UserMessageContent::Text(text) => Ok(Some(vec![bedrock_text_block(&text)])),
        UserMessageContent::Blocks(blocks) => {
            let mut content = Vec::new();
            for block in blocks {
                content.push(match block {
                    UserContentBlock::Text(text) => bedrock_text_block(&text.text),
                    UserContentBlock::Image(image) => {
                        bedrock_image_block(&image.mime_type, image.data)?
                    }
                });
            }
            Ok((!content.is_empty()).then_some(content))
        }
    }
}

fn bedrock_tool_result_block(
    tool_result: crate::conversation::ToolResultMessage,
) -> Result<BedrockContentBlock, String> {
    let mut content = Vec::new();
    for block in tool_result.content {
        content.push(match block {
            UserContentBlock::Text(TextContent { text, .. }) => bedrock_text_block(&text),
            UserContentBlock::Image(image) => bedrock_image_block(&image.mime_type, image.data)?,
        });
    }
    Ok(BedrockContentBlock {
        text: None,
        image: None,
        tool_use: None,
        tool_result: Some(BedrockToolResult {
            tool_use_id: tool_result.tool_call_id,
            content,
            status: if tool_result.is_error {
                BedrockToolResultStatus::Error
            } else {
                BedrockToolResultStatus::Success
            },
        }),
        reasoning_content: None,
        cache_point: None,
    })
}

fn bedrock_text_block(text: &str) -> BedrockContentBlock {
    BedrockContentBlock {
        text: Some(crate::utils::sanitize_surrogates(text)),
        image: None,
        tool_use: None,
        tool_result: None,
        reasoning_content: None,
        cache_point: None,
    }
}

fn bedrock_image_block(mime_type: &str, data: String) -> Result<BedrockContentBlock, String> {
    let Some(format) = bedrock_image_format(mime_type) else {
        return Err(format!("Unknown image type: {mime_type}"));
    };
    Ok(BedrockContentBlock {
        text: None,
        image: Some(BedrockImageBlock {
            format,
            source: BedrockImageSource { bytes: data },
        }),
        tool_use: None,
        tool_result: None,
        reasoning_content: None,
        cache_point: None,
    })
}

fn bedrock_reasoning_block(text: &str, signature: Option<String>) -> BedrockContentBlock {
    BedrockContentBlock {
        text: None,
        image: None,
        tool_use: None,
        tool_result: None,
        reasoning_content: Some(BedrockReasoningContent {
            reasoning_text: BedrockReasoningText {
                text: crate::utils::sanitize_surrogates(text),
                signature,
            },
        }),
        cache_point: None,
    }
}

fn normalize_bedrock_tool_call_id_for_model(
    id: &str,
    _target_model: &Model,
    _source: &crate::conversation::RichAssistantMessage,
) -> String {
    normalize_bedrock_tool_call_id(id)
}
