use serde_json::Value;

use crate::conversation::{
    AssistantContentBlock, RichAssistantMessage, TextContent, ThinkingContent, ToolCall,
};
use crate::providers::openai_completions_types::{
    OpenAiCompletionsProcessedEvent, OpenAiCompletionsStreamChunk, OpenAiCompletionsStreamDelta,
    OpenAiCompletionsStreamProcessResult, OpenAiCompletionsToolCallDelta, OpenAiCompletionsUsage,
};
use crate::types::{AssistantStopReason, Model, StreamEvent, StreamToolCall, Usage, UsageCost};
use crate::utils::parse_streaming_json;

pub fn parse_openai_completions_stream_chunks_from_value(
    value: Value,
) -> Result<Vec<OpenAiCompletionsStreamChunk>, String> {
    let chunks = serde_json::from_value::<Vec<Option<OpenAiCompletionsStreamChunk>>>(value)
        .map_err(|error| error.to_string())?;
    Ok(chunks.into_iter().flatten().collect())
}

pub fn process_openai_completions_stream_chunks<F>(
    chunks: &[OpenAiCompletionsStreamChunk],
    mut assistant: RichAssistantMessage,
    model: &Model,
    mut apply_pricing: Option<F>,
) -> Result<OpenAiCompletionsStreamProcessResult, String>
where
    F: FnMut(&mut Usage),
{
    let mut events = Vec::new();
    let mut text_index: Option<usize> = None;
    let mut thinking_index: Option<usize> = None;
    let mut has_finish_reason = false;
    let mut tool_states: Vec<OpenAiToolCallStreamState> = Vec::new();

    for chunk in chunks {
        if assistant.response_id.is_none() {
            assistant.response_id = chunk.id.clone();
        }
        if assistant.response_model.is_none() {
            if let Some(response_model) = chunk
                .model
                .as_deref()
                .filter(|response_model| *response_model != model.id)
            {
                assistant.response_model = Some(response_model.to_string());
            }
        }
        if let Some(usage) = &chunk.usage {
            let mut usage = usage_from_openai_completions_usage(usage);
            if let Some(apply_pricing) = apply_pricing.as_mut() {
                apply_pricing(&mut usage);
            }
            assistant.usage = usage.clone();
            events.push(OpenAiCompletionsProcessedEvent::Usage { usage });
        }

        let Some(choice) = chunk.choices.first() else {
            continue;
        };
        if chunk.usage.is_none() {
            if let Some(usage) = &choice.usage {
                let mut usage = usage_from_openai_completions_usage(usage);
                if let Some(apply_pricing) = apply_pricing.as_mut() {
                    apply_pricing(&mut usage);
                }
                assistant.usage = usage.clone();
                events.push(OpenAiCompletionsProcessedEvent::Usage { usage });
            }
        }
        if let Some(reason) = choice.finish_reason.as_deref() {
            let (stop_reason, error_message) = map_openai_completions_stop_reason(reason);
            assistant.stop_reason = stop_reason;
            assistant.error_message = error_message;
            has_finish_reason = true;
        }
        let Some(delta) = &choice.delta else {
            continue;
        };
        if let Some(text) = delta.content.as_deref().filter(|text| !text.is_empty()) {
            let index = ensure_openai_text_block(&mut assistant, &mut text_index, &mut events);
            if let Some(AssistantContentBlock::Text(block)) = assistant.content.get_mut(index) {
                block.text.push_str(text);
                events.push(OpenAiCompletionsProcessedEvent::TextDelta {
                    content_index: index,
                    delta: text.to_string(),
                });
            }
        }
        if let Some((field, reasoning)) = first_openai_reasoning_delta(delta) {
            let signature = if model.provider == "opencode-go" && field == "reasoning" {
                "reasoning_content"
            } else {
                field
            };
            let index = ensure_openai_thinking_block(
                &mut assistant,
                &mut thinking_index,
                &mut events,
                signature,
            );
            if let Some(AssistantContentBlock::Thinking(block)) = assistant.content.get_mut(index) {
                block.thinking.push_str(reasoning);
                events.push(OpenAiCompletionsProcessedEvent::ThinkingDelta {
                    content_index: index,
                    delta: reasoning.to_string(),
                });
            }
        }
        if let Some(tool_calls) = &delta.tool_calls {
            for tool_call in tool_calls {
                let index = ensure_openai_tool_call_block(
                    &mut assistant,
                    &mut tool_states,
                    tool_call,
                    &mut events,
                );
                let state = tool_states
                    .iter_mut()
                    .find(|state| state.content_index == index)
                    .expect("tool state should exist");
                if let Some(AssistantContentBlock::ToolCall(block)) =
                    assistant.content.get_mut(index)
                {
                    if block.id.is_empty() {
                        if let Some(id) = &tool_call.id {
                            block.id = id.clone();
                        }
                    }
                    if block.name.is_empty() {
                        if let Some(name) = tool_call
                            .function
                            .as_ref()
                            .and_then(|function| function.name.clone())
                        {
                            block.name = name;
                        }
                    }
                    let arg_delta = tool_call
                        .function
                        .as_ref()
                        .and_then(|function| function.arguments.as_deref())
                        .unwrap_or_default();
                    if !arg_delta.is_empty() {
                        state.partial_arguments.push_str(arg_delta);
                        block.arguments = parse_openai_tool_arguments(&state.partial_arguments);
                    }
                    events.push(OpenAiCompletionsProcessedEvent::ToolCallDelta {
                        content_index: index,
                        delta: arg_delta.to_string(),
                    });
                }
            }
        }
        if let Some(reasoning_details) = &delta.reasoning_details {
            for detail in reasoning_details {
                if detail.get("type").and_then(Value::as_str) != Some("reasoning.encrypted") {
                    continue;
                }
                let Some(id) = detail.get("id").and_then(Value::as_str) else {
                    continue;
                };
                if detail.get("data").is_none() {
                    continue;
                }
                if let Some(AssistantContentBlock::ToolCall(tool_call)) =
                    assistant.content.iter_mut().find(|block| match block {
                        AssistantContentBlock::ToolCall(tool_call) => tool_call.id == id,
                        _ => false,
                    })
                {
                    tool_call.thought_signature = serde_json::to_string(detail).ok();
                }
            }
        }
    }

    finish_openai_stream_block(&mut assistant, text_index, &mut events);
    finish_openai_stream_block(&mut assistant, thinking_index, &mut events);
    for state in tool_states {
        if let Some(AssistantContentBlock::ToolCall(tool_call)) =
            assistant.content.get_mut(state.content_index)
        {
            tool_call.arguments = parse_openai_tool_arguments(&state.partial_arguments);
            events.push(OpenAiCompletionsProcessedEvent::ToolCallEnd {
                content_index: state.content_index,
                tool_call: tool_call.clone(),
            });
        }
    }
    if !has_finish_reason {
        let message = "Stream ended without finish_reason".to_string();
        events.push(OpenAiCompletionsProcessedEvent::Error {
            message: message.clone(),
        });
        return Err(message);
    }
    events.push(OpenAiCompletionsProcessedEvent::Completed {
        stop_reason: assistant.stop_reason.clone(),
    });

    Ok(OpenAiCompletionsStreamProcessResult { assistant, events })
}

pub fn openai_completions_stream_events_from_process_result(
    result: OpenAiCompletionsStreamProcessResult,
) -> Result<Vec<StreamEvent>, String> {
    let mut events = Vec::new();
    let mut has_tool_calls = false;

    for event in &result.events {
        match event {
            OpenAiCompletionsProcessedEvent::TextStart { .. }
            | OpenAiCompletionsProcessedEvent::TextEnd { .. }
            | OpenAiCompletionsProcessedEvent::Completed { .. } => {}
            OpenAiCompletionsProcessedEvent::TextDelta { delta, .. } => {
                events.push(StreamEvent::TextDelta {
                    text: delta.clone(),
                });
            }
            OpenAiCompletionsProcessedEvent::ThinkingStart { content_index } => {
                events.push(StreamEvent::ThinkingStart {
                    content_index: *content_index,
                });
            }
            OpenAiCompletionsProcessedEvent::ThinkingDelta {
                content_index,
                delta,
            } => {
                events.push(StreamEvent::ThinkingDelta {
                    content_index: *content_index,
                    delta: delta.clone(),
                });
            }
            OpenAiCompletionsProcessedEvent::ThinkingEnd {
                content_index,
                content,
            } => {
                let (thinking_signature, redacted) = result
                    .assistant
                    .content
                    .get(*content_index)
                    .and_then(|block| match block {
                        AssistantContentBlock::Thinking(thinking) => {
                            Some((thinking.thinking_signature.clone(), thinking.redacted))
                        }
                        _ => None,
                    })
                    .unwrap_or((None, false));
                events.push(StreamEvent::ThinkingEnd {
                    content_index: *content_index,
                    content: content.clone(),
                    thinking_signature,
                    redacted,
                });
            }
            OpenAiCompletionsProcessedEvent::ToolCallStart { content_index } => {
                has_tool_calls = true;
                events.push(StreamEvent::ToolCallStart {
                    content_index: *content_index,
                });
            }
            OpenAiCompletionsProcessedEvent::ToolCallDelta {
                content_index,
                delta,
            } => {
                events.push(StreamEvent::ToolCallDelta {
                    content_index: *content_index,
                    delta: delta.clone(),
                });
            }
            OpenAiCompletionsProcessedEvent::ToolCallEnd {
                content_index,
                tool_call,
            } => {
                has_tool_calls = true;
                events.push(StreamEvent::ToolCallEnd {
                    content_index: *content_index,
                    tool_call: StreamToolCall {
                        id: tool_call.id.clone(),
                        name: tool_call.name.clone(),
                        arguments: tool_call.arguments.clone(),
                        thought_signature: tool_call.thought_signature.clone(),
                    },
                });
            }
            OpenAiCompletionsProcessedEvent::Usage { usage } => {
                events.push(StreamEvent::Usage {
                    usage: usage.clone(),
                });
            }
            OpenAiCompletionsProcessedEvent::Error { message } => {
                events.push(StreamEvent::Error {
                    message: message.clone(),
                });
                return Ok(events);
            }
        }
    }

    let content = openai_completions_assistant_text(&result.assistant);
    if content.is_empty() && !has_tool_calls {
        return Err("OpenAI Completions 输出文本缺失".to_string());
    }
    events.push(StreamEvent::RichFinished {
        message: result.assistant,
    });
    Ok(events)
}

fn openai_completions_assistant_text(assistant: &RichAssistantMessage) -> String {
    assistant
        .content
        .iter()
        .filter_map(|block| match block {
            AssistantContentBlock::Text(text) => Some(text.text.as_str()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("")
}

fn ensure_openai_text_block(
    assistant: &mut RichAssistantMessage,
    text_index: &mut Option<usize>,
    events: &mut Vec<OpenAiCompletionsProcessedEvent>,
) -> usize {
    if let Some(index) = *text_index {
        return index;
    }
    assistant
        .content
        .push(AssistantContentBlock::Text(TextContent {
            text: String::new(),
            text_signature: None,
        }));
    let index = assistant.content.len() - 1;
    *text_index = Some(index);
    events.push(OpenAiCompletionsProcessedEvent::TextStart {
        content_index: index,
    });
    index
}

fn ensure_openai_thinking_block(
    assistant: &mut RichAssistantMessage,
    thinking_index: &mut Option<usize>,
    events: &mut Vec<OpenAiCompletionsProcessedEvent>,
    signature: &str,
) -> usize {
    if let Some(index) = *thinking_index {
        return index;
    }
    assistant
        .content
        .push(AssistantContentBlock::Thinking(ThinkingContent {
            thinking: String::new(),
            thinking_signature: Some(signature.to_string()),
            redacted: false,
        }));
    let index = assistant.content.len() - 1;
    *thinking_index = Some(index);
    events.push(OpenAiCompletionsProcessedEvent::ThinkingStart {
        content_index: index,
    });
    index
}

fn ensure_openai_tool_call_block(
    assistant: &mut RichAssistantMessage,
    tool_states: &mut Vec<OpenAiToolCallStreamState>,
    tool_call: &OpenAiCompletionsToolCallDelta,
    events: &mut Vec<OpenAiCompletionsProcessedEvent>,
) -> usize {
    if let Some(index) = tool_call.index {
        if let Some(state) = tool_states
            .iter()
            .find(|state| state.stream_index == Some(index))
        {
            return state.content_index;
        }
    }
    if let Some(id) = &tool_call.id {
        if let Some((index, _)) = assistant.content.iter().enumerate().find(
            |(_, block)| matches!(block, AssistantContentBlock::ToolCall(tool) if tool.id == *id),
        ) {
            return index;
        }
    }
    assistant
        .content
        .push(AssistantContentBlock::ToolCall(ToolCall {
            id: tool_call.id.clone().unwrap_or_default(),
            name: tool_call
                .function
                .as_ref()
                .and_then(|function| function.name.clone())
                .unwrap_or_default(),
            arguments: Default::default(),
            thought_signature: None,
        }));
    let content_index = assistant.content.len() - 1;
    tool_states.push(OpenAiToolCallStreamState {
        stream_index: tool_call.index,
        content_index,
        partial_arguments: String::new(),
    });
    events.push(OpenAiCompletionsProcessedEvent::ToolCallStart { content_index });
    content_index
}

fn finish_openai_stream_block(
    assistant: &mut RichAssistantMessage,
    index: Option<usize>,
    events: &mut Vec<OpenAiCompletionsProcessedEvent>,
) {
    let Some(index) = index else {
        return;
    };
    match assistant.content.get(index) {
        Some(AssistantContentBlock::Text(text)) => {
            events.push(OpenAiCompletionsProcessedEvent::TextEnd {
                content_index: index,
                content: text.text.clone(),
            });
        }
        Some(AssistantContentBlock::Thinking(thinking)) => {
            events.push(OpenAiCompletionsProcessedEvent::ThinkingEnd {
                content_index: index,
                content: thinking.thinking.clone(),
            });
        }
        _ => {}
    }
}

fn first_openai_reasoning_delta(
    delta: &OpenAiCompletionsStreamDelta,
) -> Option<(&'static str, &str)> {
    [
        ("reasoning_content", delta.reasoning_content.as_deref()),
        ("reasoning", delta.reasoning.as_deref()),
        ("reasoning_text", delta.reasoning_text.as_deref()),
    ]
    .into_iter()
    .find_map(|(field, value)| {
        value
            .filter(|value| !value.is_empty())
            .map(|value| (field, value))
    })
}

fn parse_openai_tool_arguments(arguments: &str) -> std::collections::BTreeMap<String, Value> {
    match parse_streaming_json(Some(arguments)) {
        Value::Object(map) => map.into_iter().collect(),
        _ => Default::default(),
    }
}

fn usage_from_openai_completions_usage(raw: &OpenAiCompletionsUsage) -> Usage {
    let prompt_tokens = raw.prompt_tokens.unwrap_or_default();
    let cache_read = raw
        .prompt_tokens_details
        .as_ref()
        .and_then(|details| details.cached_tokens)
        .or(raw.prompt_cache_hit_tokens)
        .unwrap_or_default();
    let cache_write = raw
        .prompt_tokens_details
        .as_ref()
        .and_then(|details| details.cache_write_tokens)
        .unwrap_or_default();
    let output = raw.completion_tokens.unwrap_or_default();
    let input = prompt_tokens
        .saturating_sub(cache_read)
        .saturating_sub(cache_write);
    Usage {
        input,
        output,
        cache_read,
        cache_write,
        total_tokens: input + output + cache_read + cache_write,
        cost: UsageCost::default(),
    }
}

fn map_openai_completions_stop_reason(reason: &str) -> (AssistantStopReason, Option<String>) {
    match reason {
        "stop" | "end" => (AssistantStopReason::Stop, None),
        "length" => (AssistantStopReason::Length, None),
        "function_call" | "tool_calls" => (AssistantStopReason::ToolUse, None),
        "content_filter" => (
            AssistantStopReason::Error,
            Some("Provider finish_reason: content_filter".to_string()),
        ),
        "network_error" => (
            AssistantStopReason::Error,
            Some("Provider finish_reason: network_error".to_string()),
        ),
        other => (
            AssistantStopReason::Error,
            Some(format!("Provider finish_reason: {other}")),
        ),
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct OpenAiToolCallStreamState {
    stream_index: Option<usize>,
    content_index: usize,
    partial_arguments: String,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::providers::openai_completions_types::{
        OpenAiCompletionsFunctionDelta, OpenAiCompletionsPromptTokensDetails,
        OpenAiCompletionsStreamChoice, OpenAiCompletionsStreamDelta,
        OpenAiCompletionsToolCallDelta,
    };
    use crate::types::StreamEvent;
    use serde_json::json;

    #[test]
    fn processes_text_reasoning_tool_usage_and_finish_reason() {
        let model = model("gpt-4o");
        let mut assistant = assistant_defaults();
        assistant.content.clear();
        let chunks = vec![
            OpenAiCompletionsStreamChunk {
                id: Some("chatcmpl-1".to_string()),
                model: Some("gpt-4o".to_string()),
                choices: vec![OpenAiCompletionsStreamChoice {
                    delta: Some(OpenAiCompletionsStreamDelta {
                        content: Some("hel".to_string()),
                        ..OpenAiCompletionsStreamDelta::default()
                    }),
                    finish_reason: None,
                    usage: None,
                }],
                usage: None,
            },
            OpenAiCompletionsStreamChunk {
                id: Some("chatcmpl-1".to_string()),
                model: Some("gpt-4o".to_string()),
                choices: vec![OpenAiCompletionsStreamChoice {
                    delta: Some(OpenAiCompletionsStreamDelta {
                        content: Some("lo".to_string()),
                        reasoning_content: Some("think".to_string()),
                        tool_calls: Some(vec![OpenAiCompletionsToolCallDelta {
                            index: Some(0),
                            id: Some("tool-1".to_string()),
                            function: Some(OpenAiCompletionsFunctionDelta {
                                name: Some("read_file".to_string()),
                                arguments: Some("{\"path\":\"/tmp".to_string()),
                            }),
                        }]),
                        ..OpenAiCompletionsStreamDelta::default()
                    }),
                    finish_reason: None,
                    usage: None,
                }],
                usage: None,
            },
            OpenAiCompletionsStreamChunk {
                id: Some("chatcmpl-1".to_string()),
                model: Some("gpt-4o".to_string()),
                choices: vec![OpenAiCompletionsStreamChoice {
                    delta: Some(OpenAiCompletionsStreamDelta {
                        tool_calls: Some(vec![OpenAiCompletionsToolCallDelta {
                            index: Some(0),
                            id: None,
                            function: Some(OpenAiCompletionsFunctionDelta {
                                name: None,
                                arguments: Some("/a\"}".to_string()),
                            }),
                        }]),
                        reasoning_details: Some(vec![json!({
                            "type": "reasoning.encrypted",
                            "id": "tool-1",
                            "data": "encrypted"
                        })]),
                        ..OpenAiCompletionsStreamDelta::default()
                    }),
                    finish_reason: Some("tool_calls".to_string()),
                    usage: None,
                }],
                usage: Some(OpenAiCompletionsUsage {
                    prompt_tokens: Some(10),
                    completion_tokens: Some(5),
                    prompt_cache_hit_tokens: None,
                    prompt_tokens_details: Some(OpenAiCompletionsPromptTokensDetails {
                        cached_tokens: Some(3),
                        cache_write_tokens: Some(2),
                    }),
                }),
            },
        ];

        let result = process_openai_completions_stream_chunks(
            &chunks,
            assistant,
            &model,
            Some(|usage: &mut Usage| {
                usage.cost.total = 1.0;
            }),
        )
        .expect("stream");

        assert_eq!(result.assistant.response_id.as_deref(), Some("chatcmpl-1"));
        assert_eq!(result.assistant.stop_reason, AssistantStopReason::ToolUse);
        assert_eq!(result.assistant.usage.input, 5);
        assert_eq!(result.assistant.usage.cache_read, 3);
        assert_eq!(result.assistant.usage.cache_write, 2);
        assert_eq!(result.assistant.usage.cost.total, 1.0);
        assert!(result.events.iter().any(|event| matches!(
            event,
            OpenAiCompletionsProcessedEvent::TextEnd { content, .. } if content == "hello"
        )));
        assert!(result.events.iter().any(|event| matches!(
            event,
            OpenAiCompletionsProcessedEvent::ThinkingEnd { content, .. } if content == "think"
        )));
        let tool_call = result
            .assistant
            .content
            .iter()
            .find_map(|block| match block {
                AssistantContentBlock::ToolCall(tool_call) => Some(tool_call),
                _ => None,
            })
            .expect("tool call");
        assert_eq!(tool_call.arguments["path"], "/tmp/a");
        assert!(tool_call.thought_signature.is_some());
    }

    #[test]
    fn converts_processed_events_to_public_stream_events_like_pi() {
        let result = OpenAiCompletionsStreamProcessResult {
            assistant: RichAssistantMessage {
                content: vec![
                    AssistantContentBlock::Thinking(ThinkingContent {
                        thinking: "think".to_string(),
                        thinking_signature: Some("reasoning_content".to_string()),
                        redacted: false,
                    }),
                    AssistantContentBlock::Text(TextContent {
                        text: "hello".to_string(),
                        text_signature: None,
                    }),
                ],
                ..assistant_defaults()
            },
            events: vec![
                OpenAiCompletionsProcessedEvent::ThinkingStart { content_index: 0 },
                OpenAiCompletionsProcessedEvent::ThinkingDelta {
                    content_index: 0,
                    delta: "think".to_string(),
                },
                OpenAiCompletionsProcessedEvent::ThinkingEnd {
                    content_index: 0,
                    content: "think".to_string(),
                },
                OpenAiCompletionsProcessedEvent::TextStart { content_index: 1 },
                OpenAiCompletionsProcessedEvent::TextDelta {
                    content_index: 1,
                    delta: "hello".to_string(),
                },
                OpenAiCompletionsProcessedEvent::Usage {
                    usage: Usage {
                        input: 2,
                        output: 3,
                        total_tokens: 5,
                        ..Usage::default()
                    },
                },
                OpenAiCompletionsProcessedEvent::Completed {
                    stop_reason: AssistantStopReason::Stop,
                },
            ],
        };

        let events =
            openai_completions_stream_events_from_process_result(result).expect("public events");

        assert!(matches!(
            &events[0],
            StreamEvent::ThinkingStart { content_index: 0 }
        ));
        assert!(matches!(
            &events[2],
            StreamEvent::ThinkingEnd {
                content_index: 0,
                content,
                thinking_signature,
                redacted: false,
            } if content == "think"
                && thinking_signature.as_deref() == Some("reasoning_content")
        ));
        assert!(matches!(
            &events[3],
            StreamEvent::TextDelta { text } if text == "hello"
        ));
        assert!(matches!(
            &events[4],
            StreamEvent::Usage { usage }
                if usage.input == 2 && usage.output == 3 && usage.total_tokens == 5
        ));
        assert!(matches!(
            events.last(),
            Some(StreamEvent::RichFinished { message }) if crate::stream::rich_assistant_text(message) == "hello"
        ));
    }

    #[test]
    fn errors_when_stream_has_no_finish_reason() {
        let model = model("gpt-4o");
        let mut assistant = assistant_defaults();
        assistant.content.clear();

        let error = process_openai_completions_stream_chunks(
            &[OpenAiCompletionsStreamChunk {
                id: None,
                model: None,
                choices: vec![OpenAiCompletionsStreamChoice {
                    delta: Some(OpenAiCompletionsStreamDelta {
                        content: Some("hello".to_string()),
                        ..OpenAiCompletionsStreamDelta::default()
                    }),
                    finish_reason: None,
                    usage: None,
                }],
                usage: None,
            }],
            assistant,
            &model,
            None::<fn(&mut Usage)>,
        )
        .expect_err("missing finish reason");

        assert_eq!(error, "Stream ended without finish_reason");
    }

    #[test]
    fn parses_nullable_stream_chunks_and_ignores_null_chunks_like_pi() {
        let chunks = parse_openai_completions_stream_chunks_from_value(json!([
            null,
            {
                "id": "chatcmpl-test",
                "choices": [{
                    "delta": { "content": "OK" },
                    "finish_reason": null
                }]
            },
            {
                "id": "chatcmpl-test",
                "choices": [{
                    "delta": {},
                    "finish_reason": "stop"
                }],
                "usage": {
                    "prompt_tokens": 3,
                    "completion_tokens": 1,
                    "prompt_tokens_details": { "cached_tokens": 0 }
                }
            }
        ]))
        .expect("chunks parse");

        assert_eq!(chunks.len(), 2);
        let result = process_openai_completions_stream_chunks(
            &chunks,
            assistant_defaults(),
            &model("gpt-4o"),
            None::<fn(&mut Usage)>,
        )
        .expect("stream");

        assert_eq!(
            result.assistant.response_id.as_deref(),
            Some("chatcmpl-test")
        );
        assert_eq!(result.assistant.stop_reason, AssistantStopReason::Stop);
        assert_eq!(result.assistant.usage.total_tokens, 4);
        assert_eq!(
            result.assistant.content,
            vec![AssistantContentBlock::Text(TextContent {
                text: "OK".to_string(),
                text_signature: None,
            })]
        );
    }

    #[test]
    fn accumulates_mixed_content_reasoning_and_parallel_tool_calls_like_pi() {
        let chunks = parse_openai_completions_stream_chunks_from_value(json!([
            {
                "id": "chatcmpl-mixed-deltas",
                "choices": [{
                    "delta": {
                        "content": "answer 1",
                        "reasoning_content": "think 1",
                        "tool_calls": [
                            {
                                "index": 0,
                                "id": "tc_read_initial",
                                "function": { "name": "read", "arguments": "{\"path\":\"README" }
                            },
                            {
                                "index": 1,
                                "id": "tc_grep_initial",
                                "function": { "name": "grep", "arguments": "{\"pattern\":\"TODO" }
                            },
                            {
                                "id": "tc_list_no_index",
                                "function": { "name": "list", "arguments": "{\"path\":\"packages" }
                            },
                            {
                                "id": "tc_write_no_index",
                                "function": { "name": "write", "arguments": "{\"path\":\"out" }
                            }
                        ]
                    },
                    "finish_reason": null
                }]
            },
            {
                "id": "chatcmpl-mixed-deltas",
                "choices": [{
                    "delta": {
                        "content": " answer 2",
                        "tool_calls": [
                            {
                                "index": 1,
                                "id": "tc_grep_changed",
                                "function": { "arguments": "\",\"path\":\"src" }
                            },
                            {
                                "id": "tc_write_no_index",
                                "function": { "arguments": ".txt\",\"content\":\"ok\"}" }
                            },
                            {
                                "id": "tc_list_no_index",
                                "function": { "arguments": "/ai\"}" }
                            }
                        ]
                    },
                    "finish_reason": null
                }]
            },
            {
                "id": "chatcmpl-mixed-deltas",
                "choices": [{
                    "delta": {
                        "content": "\n",
                        "reasoning_content": " think 2",
                        "tool_calls": [
                            {
                                "index": 0,
                                "id": "tc_read_changed",
                                "function": { "arguments": ".md\"}" }
                            },
                            {
                                "index": 1,
                                "function": { "arguments": "\"}" }
                            }
                        ]
                    },
                    "finish_reason": "tool_calls"
                }],
                "usage": {
                    "prompt_tokens": 10,
                    "completion_tokens": 8,
                    "prompt_tokens_details": { "cached_tokens": 0 }
                }
            }
        ]))
        .expect("chunks parse");

        let result = process_openai_completions_stream_chunks(
            &chunks,
            assistant_defaults(),
            &model("gpt-4o"),
            None::<fn(&mut Usage)>,
        )
        .expect("stream");

        assert_eq!(result.assistant.stop_reason, AssistantStopReason::ToolUse);
        assert_eq!(event_count(&result.events, "text_start"), 1);
        assert_eq!(event_count(&result.events, "text_delta"), 3);
        assert_eq!(event_count(&result.events, "text_end"), 1);
        assert_eq!(event_count(&result.events, "thinking_start"), 1);
        assert_eq!(event_count(&result.events, "thinking_delta"), 2);
        assert_eq!(event_count(&result.events, "thinking_end"), 1);
        assert_eq!(event_count(&result.events, "tool_start"), 4);
        assert_eq!(event_count(&result.events, "tool_delta"), 9);
        assert_eq!(event_count(&result.events, "tool_end"), 4);

        assert_eq!(result.assistant.content.len(), 6);
        assert_eq!(
            result.assistant.content[0],
            AssistantContentBlock::Text(TextContent {
                text: "answer 1 answer 2\n".to_string(),
                text_signature: None,
            })
        );
        assert_eq!(
            result.assistant.content[1],
            AssistantContentBlock::Thinking(ThinkingContent {
                thinking: "think 1 think 2".to_string(),
                thinking_signature: Some("reasoning_content".to_string()),
                redacted: false,
            })
        );

        assert_tool_call(
            &result.assistant.content[2],
            "tc_read_initial",
            "read",
            "path",
            "README.md",
        );
        assert_tool_call(
            &result.assistant.content[3],
            "tc_grep_initial",
            "grep",
            "pattern",
            "TODO",
        );
        assert_eq!(
            tool_call(&result.assistant.content[3]).arguments["path"],
            json!("src")
        );
        assert_tool_call(
            &result.assistant.content[4],
            "tc_list_no_index",
            "list",
            "path",
            "packages/ai",
        );
        assert_tool_call(
            &result.assistant.content[5],
            "tc_write_no_index",
            "write",
            "path",
            "out.txt",
        );
        assert_eq!(
            tool_call(&result.assistant.content[5]).arguments["content"],
            json!("ok")
        );
    }

    fn event_count(events: &[OpenAiCompletionsProcessedEvent], kind: &str) -> usize {
        events
            .iter()
            .filter(|event| {
                matches!(
                    (kind, event),
                    (
                        "text_start",
                        OpenAiCompletionsProcessedEvent::TextStart { .. }
                    ) | (
                        "text_delta",
                        OpenAiCompletionsProcessedEvent::TextDelta { .. }
                    ) | ("text_end", OpenAiCompletionsProcessedEvent::TextEnd { .. })
                        | (
                            "thinking_start",
                            OpenAiCompletionsProcessedEvent::ThinkingStart { .. }
                        )
                        | (
                            "thinking_delta",
                            OpenAiCompletionsProcessedEvent::ThinkingDelta { .. }
                        )
                        | (
                            "thinking_end",
                            OpenAiCompletionsProcessedEvent::ThinkingEnd { .. }
                        )
                        | (
                            "tool_start",
                            OpenAiCompletionsProcessedEvent::ToolCallStart { .. }
                        )
                        | (
                            "tool_delta",
                            OpenAiCompletionsProcessedEvent::ToolCallDelta { .. }
                        )
                        | (
                            "tool_end",
                            OpenAiCompletionsProcessedEvent::ToolCallEnd { .. }
                        )
                )
            })
            .count()
    }

    fn assert_tool_call(
        block: &AssistantContentBlock,
        id: &str,
        name: &str,
        arg_name: &str,
        arg_value: &str,
    ) {
        let tool_call = tool_call(block);
        assert_eq!(tool_call.id, id);
        assert_eq!(tool_call.name, name);
        assert_eq!(tool_call.arguments[arg_name], json!(arg_value));
    }

    fn tool_call(block: &AssistantContentBlock) -> &ToolCall {
        let AssistantContentBlock::ToolCall(tool_call) = block else {
            panic!("tool call");
        };
        tool_call
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
}
