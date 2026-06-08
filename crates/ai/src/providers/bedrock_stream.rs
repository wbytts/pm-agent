use std::collections::BTreeMap;

use serde_json::Value;

use crate::conversation::{
    AssistantContentBlock, RichAssistantMessage, TextContent, ThinkingContent, ToolCall,
};
use crate::providers::bedrock_types::{
    BedrockContentBlockDelta, BedrockContentBlockStart, BedrockConversationRole,
    BedrockProcessedEvent, BedrockStreamEvent, BedrockStreamProcessResult, BedrockUsage,
};
use crate::types::{AssistantStopReason, StreamEvent, StreamToolCall, Usage, UsageCost};
use crate::utils::parse_streaming_json;

pub fn map_bedrock_stop_reason(reason: Option<&str>) -> AssistantStopReason {
    match reason {
        Some("end_turn") | Some("stop_sequence") => AssistantStopReason::Stop,
        Some("max_tokens") | Some("model_context_window_exceeded") => AssistantStopReason::Length,
        Some("tool_use") => AssistantStopReason::ToolUse,
        _ => AssistantStopReason::Error,
    }
}

pub fn process_bedrock_stream_events<F>(
    events: &[BedrockStreamEvent],
    mut assistant: RichAssistantMessage,
    mut apply_pricing: Option<F>,
) -> Result<BedrockStreamProcessResult, String>
where
    F: FnMut(&mut Usage),
{
    let mut processed_events = Vec::new();
    let mut stream_blocks: Vec<BedrockStreamBlockState> = Vec::new();

    for event in events {
        match event {
            BedrockStreamEvent::MessageStart { role } => {
                if *role != BedrockConversationRole::Assistant {
                    let message =
                        "Unexpected assistant message start but got user message start instead"
                            .to_string();
                    processed_events.push(BedrockProcessedEvent::Error {
                        message: message.clone(),
                    });
                    return Err(message);
                }
                processed_events.push(BedrockProcessedEvent::Start);
            }
            BedrockStreamEvent::ContentBlockStart {
                content_block_index,
                start,
            } => {
                if let Some(tool_use) = &start.tool_use {
                    assistant
                        .content
                        .push(AssistantContentBlock::ToolCall(ToolCall {
                            id: tool_use.tool_use_id.clone().unwrap_or_default(),
                            name: tool_use.name.clone().unwrap_or_default(),
                            arguments: BTreeMap::new(),
                            thought_signature: None,
                        }));
                    let content_index = assistant.content.len() - 1;
                    stream_blocks.push(BedrockStreamBlockState {
                        content_block_index: *content_block_index,
                        content_index,
                        partial_json: String::new(),
                    });
                    processed_events.push(BedrockProcessedEvent::ToolCallStart { content_index });
                }
            }
            BedrockStreamEvent::ContentBlockDelta {
                content_block_index,
                delta,
            } => {
                if let Some(text) = &delta.text {
                    let content_index = ensure_bedrock_text_block(
                        &mut assistant,
                        &mut stream_blocks,
                        *content_block_index,
                        &mut processed_events,
                    );
                    if let Some(AssistantContentBlock::Text(content)) =
                        assistant.content.get_mut(content_index)
                    {
                        content.text.push_str(text);
                        processed_events.push(BedrockProcessedEvent::TextDelta {
                            content_index,
                            delta: text.clone(),
                        });
                    }
                } else if let Some(tool_use) = &delta.tool_use {
                    if let Some(state) = stream_blocks
                        .iter_mut()
                        .find(|state| state.content_block_index == *content_block_index)
                    {
                        if let Some(AssistantContentBlock::ToolCall(tool_call)) =
                            assistant.content.get_mut(state.content_index)
                        {
                            let delta = tool_use.input.clone().unwrap_or_default();
                            state.partial_json.push_str(&delta);
                            tool_call.arguments = parse_tool_arguments(&state.partial_json);
                            processed_events.push(BedrockProcessedEvent::ToolCallDelta {
                                content_index: state.content_index,
                                delta,
                            });
                        }
                    }
                } else if let Some(reasoning) = &delta.reasoning_content {
                    let content_index = ensure_bedrock_thinking_block(
                        &mut assistant,
                        &mut stream_blocks,
                        *content_block_index,
                        &mut processed_events,
                    );
                    if let Some(AssistantContentBlock::Thinking(thinking)) =
                        assistant.content.get_mut(content_index)
                    {
                        if let Some(text) = &reasoning.text {
                            thinking.thinking.push_str(text);
                            processed_events.push(BedrockProcessedEvent::ThinkingDelta {
                                content_index,
                                delta: text.clone(),
                            });
                        }
                        if let Some(signature) = &reasoning.signature {
                            let current =
                                thinking.thinking_signature.get_or_insert_with(String::new);
                            current.push_str(signature);
                        }
                    }
                }
            }
            BedrockStreamEvent::ContentBlockStop {
                content_block_index,
            } => {
                let Some(state_index) = stream_blocks
                    .iter()
                    .position(|state| state.content_block_index == *content_block_index)
                else {
                    continue;
                };
                let state = stream_blocks.remove(state_index);
                match assistant.content.get_mut(state.content_index) {
                    Some(AssistantContentBlock::Text(text)) => {
                        processed_events.push(BedrockProcessedEvent::TextEnd {
                            content_index: state.content_index,
                            content: text.text.clone(),
                        });
                    }
                    Some(AssistantContentBlock::Thinking(thinking)) => {
                        processed_events.push(BedrockProcessedEvent::ThinkingEnd {
                            content_index: state.content_index,
                            content: thinking.thinking.clone(),
                        });
                    }
                    Some(AssistantContentBlock::ToolCall(tool_call)) => {
                        tool_call.arguments = parse_tool_arguments(&state.partial_json);
                        processed_events.push(BedrockProcessedEvent::ToolCallEnd {
                            content_index: state.content_index,
                            tool_call: tool_call.clone(),
                        });
                    }
                    None => {}
                }
            }
            BedrockStreamEvent::MessageStop { stop_reason } => {
                assistant.stop_reason = map_bedrock_stop_reason(stop_reason.as_deref());
                processed_events.push(BedrockProcessedEvent::Completed {
                    stop_reason: assistant.stop_reason.clone(),
                });
            }
            BedrockStreamEvent::Metadata { usage } => {
                let mut usage = usage_from_bedrock_usage(usage.as_ref());
                if let Some(apply_pricing) = apply_pricing.as_mut() {
                    apply_pricing(&mut usage);
                }
                assistant.usage = usage.clone();
                processed_events.push(BedrockProcessedEvent::Usage { usage });
            }
            BedrockStreamEvent::Error { name, message } => {
                let message = format_bedrock_error(name.as_deref(), message);
                processed_events.push(BedrockProcessedEvent::Error {
                    message: message.clone(),
                });
                return Err(message);
            }
        }
    }

    Ok(BedrockStreamProcessResult {
        assistant,
        events: processed_events,
    })
}

pub fn bedrock_stream_events_from_process_result(
    result: BedrockStreamProcessResult,
) -> Vec<StreamEvent> {
    let mut events = Vec::new();
    let mut text_content = String::new();

    for event in result.events {
        match event {
            BedrockProcessedEvent::Start
            | BedrockProcessedEvent::TextStart { .. }
            | BedrockProcessedEvent::TextEnd { .. }
            | BedrockProcessedEvent::Completed { .. } => {}
            BedrockProcessedEvent::TextDelta { delta, .. } => {
                text_content.push_str(&delta);
                events.push(StreamEvent::TextDelta { text: delta });
            }
            BedrockProcessedEvent::ThinkingStart { content_index } => {
                events.push(StreamEvent::ThinkingStart { content_index });
            }
            BedrockProcessedEvent::ThinkingDelta {
                content_index,
                delta,
            } => {
                events.push(StreamEvent::ThinkingDelta {
                    content_index,
                    delta,
                });
            }
            BedrockProcessedEvent::ThinkingEnd {
                content_index,
                content,
            } => {
                let thinking_signature =
                    result
                        .assistant
                        .content
                        .get(content_index)
                        .and_then(|block| match block {
                            AssistantContentBlock::Thinking(thinking) => {
                                thinking.thinking_signature.clone()
                            }
                            _ => None,
                        });
                events.push(StreamEvent::ThinkingEnd {
                    content_index,
                    content,
                    thinking_signature,
                    redacted: false,
                });
            }
            BedrockProcessedEvent::ToolCallStart { content_index } => {
                events.push(StreamEvent::ToolCallStart { content_index });
            }
            BedrockProcessedEvent::ToolCallDelta {
                content_index,
                delta,
            } => {
                events.push(StreamEvent::ToolCallDelta {
                    content_index,
                    delta,
                });
            }
            BedrockProcessedEvent::ToolCallEnd {
                content_index,
                tool_call,
            } => {
                events.push(StreamEvent::ToolCallEnd {
                    content_index,
                    tool_call: StreamToolCall {
                        id: tool_call.id,
                        name: tool_call.name,
                        arguments: tool_call.arguments,
                        thought_signature: tool_call.thought_signature,
                    },
                });
            }
            BedrockProcessedEvent::Usage { usage } => {
                events.push(StreamEvent::Usage { usage });
            }
            BedrockProcessedEvent::Error { message } => {
                events.push(StreamEvent::Error { message });
            }
        }
    }

    events.push(StreamEvent::RichFinished {
        message: result.assistant,
    });
    events
}

pub fn parse_bedrock_converse_event_stream_body(
    body: &[u8],
) -> Result<Vec<BedrockStreamEvent>, String> {
    let mut offset = 0usize;
    let mut events = Vec::new();

    while offset < body.len() {
        if body.len() - offset < 16 {
            return Err("Bedrock event stream frame 过短".to_string());
        }
        let total_len = read_u32(body, offset)? as usize;
        let headers_len = read_u32(body, offset + 4)? as usize;
        if total_len < 16 || offset + total_len > body.len() {
            return Err("Bedrock event stream frame 长度无效".to_string());
        }
        let headers_start = offset + 12;
        let headers_end = headers_start + headers_len;
        if headers_end > offset + total_len - 4 {
            return Err("Bedrock event stream headers 长度无效".to_string());
        }
        let payload_end = offset + total_len - 4;
        let event_type = parse_aws_event_stream_event_type(&body[headers_start..headers_end])?;
        let payload = &body[headers_end..payload_end];
        events.push(parse_bedrock_converse_event_payload(&event_type, payload)?);
        offset += total_len;
    }

    Ok(events)
}

fn read_u32(body: &[u8], offset: usize) -> Result<u32, String> {
    let bytes = body
        .get(offset..offset + 4)
        .ok_or_else(|| "Bedrock event stream u32 越界".to_string())?;
    Ok(u32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
}

fn parse_aws_event_stream_event_type(headers: &[u8]) -> Result<String, String> {
    let mut offset = 0usize;
    while offset < headers.len() {
        let name_len = *headers
            .get(offset)
            .ok_or_else(|| "Bedrock event stream header name 缺失".to_string())?
            as usize;
        offset += 1;
        let name = std::str::from_utf8(
            headers
                .get(offset..offset + name_len)
                .ok_or_else(|| "Bedrock event stream header name 越界".to_string())?,
        )
        .map_err(|error| format!("Bedrock event stream header name UTF-8 无效：{error}"))?;
        offset += name_len;
        let value_type = *headers
            .get(offset)
            .ok_or_else(|| "Bedrock event stream header type 缺失".to_string())?;
        offset += 1;
        match value_type {
            7 => {
                let len_bytes = headers
                    .get(offset..offset + 2)
                    .ok_or_else(|| "Bedrock event stream string header 长度缺失".to_string())?;
                let value_len = u16::from_be_bytes([len_bytes[0], len_bytes[1]]) as usize;
                offset += 2;
                let value = std::str::from_utf8(
                    headers
                        .get(offset..offset + value_len)
                        .ok_or_else(|| "Bedrock event stream string header 越界".to_string())?,
                )
                .map_err(|error| {
                    format!("Bedrock event stream string header UTF-8 无效：{error}")
                })?;
                offset += value_len;
                if name == ":event-type" {
                    return Ok(value.to_string());
                }
            }
            _ => {
                return Err(format!(
                    "Bedrock event stream 暂不支持 header 类型：{value_type}"
                ));
            }
        }
    }
    Err("Bedrock event stream 缺少 :event-type header".to_string())
}

fn parse_bedrock_converse_event_payload(
    event_type: &str,
    payload: &[u8],
) -> Result<BedrockStreamEvent, String> {
    let value: Value = serde_json::from_slice(payload)
        .map_err(|error| format!("Bedrock event stream payload JSON 无效：{error}"))?;
    match event_type {
        "messageStart" => Ok(BedrockStreamEvent::MessageStart {
            role: serde_json::from_value(
                value
                    .get("role")
                    .cloned()
                    .ok_or_else(|| "messageStart 缺少 role".to_string())?,
            )
            .map_err(|error| format!("messageStart role 无效：{error}"))?,
        }),
        "contentBlockStart" => Ok(BedrockStreamEvent::ContentBlockStart {
            content_block_index: json_u64(&value, "contentBlockIndex")? as usize,
            start: serde_json::from_value::<BedrockContentBlockStart>(
                value
                    .get("start")
                    .cloned()
                    .ok_or_else(|| "contentBlockStart 缺少 start".to_string())?,
            )
            .map_err(|error| format!("contentBlockStart start 无效：{error}"))?,
        }),
        "contentBlockDelta" => Ok(BedrockStreamEvent::ContentBlockDelta {
            content_block_index: json_u64(&value, "contentBlockIndex")? as usize,
            delta: serde_json::from_value::<BedrockContentBlockDelta>(
                value
                    .get("delta")
                    .cloned()
                    .ok_or_else(|| "contentBlockDelta 缺少 delta".to_string())?,
            )
            .map_err(|error| format!("contentBlockDelta delta 无效：{error}"))?,
        }),
        "contentBlockStop" => Ok(BedrockStreamEvent::ContentBlockStop {
            content_block_index: json_u64(&value, "contentBlockIndex")? as usize,
        }),
        "messageStop" => Ok(BedrockStreamEvent::MessageStop {
            stop_reason: value
                .get("stopReason")
                .and_then(Value::as_str)
                .map(str::to_string),
        }),
        "metadata" => Ok(BedrockStreamEvent::Metadata {
            usage: value
                .get("usage")
                .cloned()
                .map(serde_json::from_value::<BedrockUsage>)
                .transpose()
                .map_err(|error| format!("metadata usage 无效：{error}"))?,
        }),
        "internalServerException"
        | "modelStreamErrorException"
        | "validationException"
        | "throttlingException"
        | "serviceUnavailableException" => Ok(BedrockStreamEvent::Error {
            name: Some(bedrock_exception_name(event_type).to_string()),
            message: value
                .get("message")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string(),
        }),
        _ => Err(format!("未知 Bedrock event stream 事件：{event_type}")),
    }
}

fn json_u64(value: &Value, key: &str) -> Result<u64, String> {
    value
        .get(key)
        .and_then(Value::as_u64)
        .ok_or_else(|| format!("Bedrock event stream 缺少数字字段：{key}"))
}

fn bedrock_exception_name(event_type: &str) -> &'static str {
    match event_type {
        "internalServerException" => "InternalServerException",
        "modelStreamErrorException" => "ModelStreamErrorException",
        "validationException" => "ValidationException",
        "throttlingException" => "ThrottlingException",
        "serviceUnavailableException" => "ServiceUnavailableException",
        _ => "BedrockException",
    }
}

fn usage_from_bedrock_usage(usage: Option<&BedrockUsage>) -> Usage {
    let Some(usage) = usage else {
        return Usage::default();
    };
    let input = usage.input_tokens.unwrap_or_default();
    let output = usage.output_tokens.unwrap_or_default();
    Usage {
        input,
        output,
        cache_read: usage.cache_read_input_tokens.unwrap_or_default(),
        cache_write: usage.cache_write_input_tokens.unwrap_or_default(),
        total_tokens: usage.total_tokens.unwrap_or(input + output),
        cost: UsageCost::default(),
    }
}

fn format_bedrock_error(name: Option<&str>, message: &str) -> String {
    let Some(name) = name else {
        return message.to_string();
    };
    let prefix = match name {
        "InternalServerException" => "Internal server error",
        "ModelStreamErrorException" => "Model stream error",
        "ValidationException" => "Validation error",
        "ThrottlingException" => "Throttling error",
        "ServiceUnavailableException" => "Service unavailable",
        _ => name,
    };
    format!("{prefix}: {message}")
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct BedrockStreamBlockState {
    content_block_index: usize,
    content_index: usize,
    partial_json: String,
}

fn ensure_bedrock_text_block(
    assistant: &mut RichAssistantMessage,
    stream_blocks: &mut Vec<BedrockStreamBlockState>,
    content_block_index: usize,
    processed_events: &mut Vec<BedrockProcessedEvent>,
) -> usize {
    if let Some(state) = stream_blocks
        .iter()
        .find(|state| state.content_block_index == content_block_index)
    {
        return state.content_index;
    }

    assistant
        .content
        .push(AssistantContentBlock::Text(TextContent {
            text: String::new(),
            text_signature: None,
        }));
    let content_index = assistant.content.len() - 1;
    stream_blocks.push(BedrockStreamBlockState {
        content_block_index,
        content_index,
        partial_json: String::new(),
    });
    processed_events.push(BedrockProcessedEvent::TextStart { content_index });
    content_index
}

fn ensure_bedrock_thinking_block(
    assistant: &mut RichAssistantMessage,
    stream_blocks: &mut Vec<BedrockStreamBlockState>,
    content_block_index: usize,
    processed_events: &mut Vec<BedrockProcessedEvent>,
) -> usize {
    if let Some(state) = stream_blocks
        .iter()
        .find(|state| state.content_block_index == content_block_index)
    {
        return state.content_index;
    }

    assistant
        .content
        .push(AssistantContentBlock::Thinking(ThinkingContent {
            thinking: String::new(),
            thinking_signature: None,
            redacted: false,
        }));
    let content_index = assistant.content.len() - 1;
    stream_blocks.push(BedrockStreamBlockState {
        content_block_index,
        content_index,
        partial_json: String::new(),
    });
    processed_events.push(BedrockProcessedEvent::ThinkingStart { content_index });
    content_index
}

fn parse_tool_arguments(arguments: &str) -> BTreeMap<String, Value> {
    match parse_streaming_json(Some(arguments)) {
        Value::Object(map) => map.into_iter().collect(),
        _ => BTreeMap::new(),
    }
}
