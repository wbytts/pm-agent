use crate::event_stream::{create_assistant_message_event_stream, AssistantMessageEventStream};
use crate::{
    AiResult, AssistantMessage, AssistantMessageEvent, AssistantStopReason, LanguageModelProvider,
    Message, MessageRole, ProviderRegistry, StreamEvent, StreamRequest, Usage,
};
use crate::{AssistantContentBlock, TextContent, ThinkingContent, ToolCall};

pub fn stream(
    request: StreamRequest,
    providers: &ProviderRegistry,
) -> AiResult<AssistantMessageEventStream> {
    let provider = providers.provider_for(&request.model)?;
    provider_events_to_stream(provider.stream(request)?)
}

pub fn complete(
    request: StreamRequest,
    providers: &ProviderRegistry,
) -> AiResult<AssistantMessage> {
    let stream = stream(request, providers)?;
    Ok(stream
        .into_result()
        .unwrap_or_else(|| AssistantMessage::error("stream ended without final result")))
}

pub fn stream_with_builtins(request: StreamRequest) -> AiResult<AssistantMessageEventStream> {
    let providers = ProviderRegistry::builtins();
    stream(request, &providers)
}

pub fn complete_with_builtins(request: StreamRequest) -> AiResult<AssistantMessage> {
    let providers = ProviderRegistry::builtins();
    complete(request, &providers)
}

pub fn provider_events_to_stream(
    events: Vec<StreamEvent>,
) -> AiResult<AssistantMessageEventStream> {
    let mut stream = create_assistant_message_event_stream();
    let mut content = String::new();
    let mut content_blocks = Vec::<AssistantContentBlock>::new();
    let mut usage = Usage::default();
    let mut saw_final = false;
    let mut saw_tool_call = false;

    for event in events {
        match event {
            StreamEvent::ThinkingStart { content_index } => {
                ensure_thinking_content_block(&mut content_blocks, content_index);
                stream.push(AssistantMessageEvent::ThinkingStart { content_index });
            }
            StreamEvent::ThinkingDelta {
                content_index,
                delta,
            } => {
                ensure_thinking_content_block(&mut content_blocks, content_index);
                if let Some(AssistantContentBlock::Thinking(thinking)) =
                    content_blocks.get_mut(content_index)
                {
                    thinking.thinking.push_str(&delta);
                }
                stream.push(AssistantMessageEvent::ThinkingDelta {
                    content_index,
                    delta,
                });
            }
            StreamEvent::ThinkingEnd {
                content_index,
                content: thinking_content,
                thinking_signature,
                redacted,
            } => {
                ensure_thinking_content_block(&mut content_blocks, content_index);
                if let Some(AssistantContentBlock::Thinking(thinking)) =
                    content_blocks.get_mut(content_index)
                {
                    thinking.thinking = thinking_content.clone();
                    thinking.thinking_signature = thinking_signature;
                    thinking.redacted = redacted;
                }
                stream.push(AssistantMessageEvent::ThinkingEnd {
                    content_index,
                    content: thinking_content,
                });
            }
            StreamEvent::TextDelta { text } => {
                content.push_str(&text);
                append_text_content_block(&mut content_blocks, &text);
                stream.push(AssistantMessageEvent::TextDelta { text });
            }
            StreamEvent::ToolCallStart { content_index } => {
                stream.push(AssistantMessageEvent::ToolCallStart { content_index });
            }
            StreamEvent::ToolCallDelta {
                content_index,
                delta,
            } => {
                stream.push(AssistantMessageEvent::ToolCallDelta {
                    content_index,
                    delta,
                });
            }
            StreamEvent::ToolCallEnd {
                content_index,
                tool_call,
            } => {
                saw_tool_call = true;
                set_tool_call_content_block(&mut content_blocks, content_index, &tool_call);
                stream.push(AssistantMessageEvent::ToolCallEnd {
                    content_index,
                    tool_call,
                });
            }
            StreamEvent::Usage { usage: next_usage } => {
                usage = next_usage.clone();
                stream.push(AssistantMessageEvent::Usage { usage: next_usage });
            }
            StreamEvent::Finished { message } => {
                saw_final = true;
                if content.is_empty() {
                    content = message.content.clone();
                }
                stream.push(AssistantMessageEvent::Done {
                    message: AssistantMessage {
                        role: MessageRole::Assistant,
                        content: message.content,
                        content_blocks: if content_blocks.is_empty() {
                            AssistantMessage::from_text(content.clone(), usage.clone())
                                .content_blocks
                        } else {
                            content_blocks.clone()
                        },
                        response_model: None,
                        response_id: None,
                        usage: usage.clone(),
                        stop_reason: if saw_tool_call {
                            AssistantStopReason::ToolUse
                        } else {
                            AssistantStopReason::Stop
                        },
                        error_message: None,
                        diagnostics: Vec::new(),
                    },
                });
            }
            StreamEvent::RichFinished { message } => {
                saw_final = true;
                let message_content = rich_assistant_text(&message);
                if content.is_empty() {
                    content = message_content.clone();
                }
                let final_usage = if usage == Usage::default() {
                    message.usage.clone()
                } else {
                    usage.clone()
                };
                let final_content_blocks = if content_blocks.is_empty() {
                    message.content.clone()
                } else {
                    content_blocks.clone()
                };
                stream.push(AssistantMessageEvent::Done {
                    message: AssistantMessage {
                        role: MessageRole::Assistant,
                        content: if message_content.is_empty() {
                            content.clone()
                        } else {
                            message_content
                        },
                        content_blocks: final_content_blocks,
                        response_model: message.response_model,
                        response_id: message.response_id,
                        usage: final_usage,
                        stop_reason: if saw_tool_call {
                            AssistantStopReason::ToolUse
                        } else {
                            message.stop_reason
                        },
                        error_message: message.error_message,
                        diagnostics: message.diagnostics,
                    },
                });
            }
            StreamEvent::Error { message } => {
                saw_final = true;
                stream.push(AssistantMessageEvent::Error {
                    error: AssistantMessage::error(message),
                });
            }
        }
    }

    if !saw_final {
        let stop_reason = if saw_tool_call {
            AssistantStopReason::ToolUse
        } else {
            AssistantStopReason::Stop
        };
        stream.push(AssistantMessageEvent::Done {
            message: AssistantMessage {
                role: MessageRole::Assistant,
                content,
                content_blocks,
                response_model: None,
                response_id: None,
                usage,
                stop_reason,
                error_message: None,
                diagnostics: Vec::new(),
            },
        });
    }

    Ok(stream)
}

pub(crate) fn rich_assistant_text(message: &crate::RichAssistantMessage) -> String {
    message
        .content
        .iter()
        .filter_map(|block| match block {
            AssistantContentBlock::Text(text) => Some(text.text.as_str()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("")
}

fn append_text_content_block(content_blocks: &mut Vec<AssistantContentBlock>, text: &str) {
    if text.is_empty() {
        return;
    }
    match content_blocks.last_mut() {
        Some(AssistantContentBlock::Text(block)) => block.text.push_str(text),
        _ => content_blocks.push(AssistantContentBlock::Text(TextContent {
            text: text.to_string(),
            text_signature: None,
        })),
    }
}

fn ensure_thinking_content_block(
    content_blocks: &mut Vec<AssistantContentBlock>,
    content_index: usize,
) {
    while content_blocks.len() <= content_index {
        content_blocks.push(AssistantContentBlock::Text(TextContent {
            text: String::new(),
            text_signature: None,
        }));
    }
    if !matches!(
        content_blocks.get(content_index),
        Some(AssistantContentBlock::Thinking(_))
    ) {
        content_blocks[content_index] = AssistantContentBlock::Thinking(ThinkingContent {
            thinking: String::new(),
            thinking_signature: None,
            redacted: false,
        });
    }
}

fn set_tool_call_content_block(
    content_blocks: &mut Vec<AssistantContentBlock>,
    content_index: usize,
    tool_call: &crate::types::StreamToolCall,
) {
    while content_blocks.len() <= content_index {
        content_blocks.push(AssistantContentBlock::Text(TextContent {
            text: String::new(),
            text_signature: None,
        }));
    }
    content_blocks[content_index] = AssistantContentBlock::ToolCall(ToolCall {
        id: tool_call.id.clone(),
        name: tool_call.name.clone(),
        arguments: tool_call.arguments.clone(),
        thought_signature: tool_call.thought_signature.clone(),
    });
}

pub fn simple_request(model: crate::Model, prompt: impl Into<String>) -> StreamRequest {
    StreamRequest {
        model,
        messages: vec![Message {
            role: MessageRole::User,
            content: prompt.into(),
        }],
        rich_messages: Vec::new(),
        tools: Vec::new(),
        metadata: Default::default(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::StreamToolCall;
    use crate::utils::create_message_diagnostic;
    use crate::AssistantContentBlock;
    use crate::{providers::EchoProvider, ProviderRegistry};
    use crate::{Model, RegisteredProvider, RichAssistantMessage};

    fn echo_model() -> Model {
        Model {
            id: "echo".to_string(),
            provider: "local".to_string(),
            api: "local-echo".to_string(),
            display_name: "Local Echo".to_string(),
            context_window: 32_000,
            ..Model::default()
        }
    }

    #[test]
    fn converts_provider_events_to_assistant_stream() {
        let stream = provider_events_to_stream(vec![
            StreamEvent::TextDelta {
                text: "hello".to_string(),
            },
            StreamEvent::Usage {
                usage: Usage {
                    input: 1,
                    output: 2,
                    ..Usage::default()
                },
            },
            StreamEvent::Finished {
                message: Message {
                    role: MessageRole::Assistant,
                    content: "hello".to_string(),
                },
            },
        ])
        .expect("stream");

        let result = stream.result().expect("final result");
        assert_eq!(result.content, "hello");
        assert_eq!(result.usage.input, 1);
        assert_eq!(result.stop_reason, AssistantStopReason::Stop);
    }

    #[test]
    fn complete_uses_registered_provider() {
        let mut providers = ProviderRegistry::new();
        providers.register(RegisteredProvider::Echo(EchoProvider), None);
        let message = complete(simple_request(echo_model(), "hi"), &providers).expect("complete");
        assert_eq!(message.content, "hi");
    }

    #[test]
    fn stream_produces_error_result_for_error_event() {
        let stream = provider_events_to_stream(vec![StreamEvent::Error {
            message: "failed".to_string(),
        }])
        .expect("stream");
        let message = stream.into_result().expect("result");
        assert_eq!(message.stop_reason, AssistantStopReason::Error);
        assert_eq!(message.error_message.as_deref(), Some("failed"));
    }

    #[test]
    fn final_message_preserves_rich_text_and_tool_call_blocks_like_pi() {
        let mut arguments = std::collections::BTreeMap::new();
        arguments.insert("path".to_string(), serde_json::json!("README.md"));

        let stream = provider_events_to_stream(vec![
            StreamEvent::TextDelta {
                text: "I'll edit it.".to_string(),
            },
            StreamEvent::ToolCallStart { content_index: 1 },
            StreamEvent::ToolCallEnd {
                content_index: 1,
                tool_call: StreamToolCall {
                    id: "toolu_1".to_string(),
                    name: "edit".to_string(),
                    arguments,
                    thought_signature: None,
                },
            },
            StreamEvent::Finished {
                message: Message {
                    role: MessageRole::Assistant,
                    content: "I'll edit it.".to_string(),
                },
            },
        ])
        .expect("stream");

        let message = stream.result().expect("final result");
        assert_eq!(message.content, "I'll edit it.");
        assert_eq!(message.stop_reason, AssistantStopReason::ToolUse);
        assert_eq!(message.content_blocks.len(), 2);
        assert!(matches!(
            &message.content_blocks[0],
            AssistantContentBlock::Text(text) if text.text == "I'll edit it."
        ));
        assert!(matches!(
            &message.content_blocks[1],
            AssistantContentBlock::ToolCall(tool_call)
                if tool_call.id == "toolu_1"
                    && tool_call.name == "edit"
                    && tool_call.arguments["path"] == serde_json::json!("README.md")
        ));
    }

    #[test]
    fn final_message_preserves_thinking_blocks_like_pi() {
        let stream = provider_events_to_stream(vec![
            StreamEvent::ThinkingStart { content_index: 0 },
            StreamEvent::ThinkingDelta {
                content_index: 0,
                delta: "plan".to_string(),
            },
            StreamEvent::ThinkingEnd {
                content_index: 0,
                content: "plan".to_string(),
                thinking_signature: Some("sig".to_string()),
                redacted: false,
            },
            StreamEvent::TextDelta {
                text: "answer".to_string(),
            },
            StreamEvent::Finished {
                message: Message {
                    role: MessageRole::Assistant,
                    content: "answer".to_string(),
                },
            },
        ])
        .expect("stream");

        let events = stream.collect::<Vec<_>>();
        assert!(matches!(
            events[0],
            AssistantMessageEvent::ThinkingStart { content_index: 0 }
        ));
        assert!(matches!(
            &events[1],
            AssistantMessageEvent::ThinkingDelta {
                content_index: 0,
                delta
            } if delta == "plan"
        ));
        assert!(matches!(
            &events[2],
            AssistantMessageEvent::ThinkingEnd {
                content_index: 0,
                content
            } if content == "plan"
        ));

        let AssistantMessageEvent::Done { message } = events.last().expect("done") else {
            panic!("expected done event");
        };
        assert_eq!(message.content, "answer");
        assert_eq!(message.content_blocks.len(), 2);
        assert!(matches!(
            &message.content_blocks[0],
            AssistantContentBlock::Thinking(thinking)
                if thinking.thinking == "plan"
                    && thinking.thinking_signature.as_deref() == Some("sig")
                    && !thinking.redacted
        ));
        assert!(matches!(
            &message.content_blocks[1],
            AssistantContentBlock::Text(text) if text.text == "answer"
        ));
    }

    #[test]
    fn final_message_preserves_provider_response_metadata_like_pi() {
        let stream = provider_events_to_stream(vec![StreamEvent::RichFinished {
            message: RichAssistantMessage {
                provider: "openai".to_string(),
                api: "openai-completions".to_string(),
                model: "openrouter/auto".to_string(),
                content: vec![AssistantContentBlock::Text(TextContent {
                    text: "answer".to_string(),
                    text_signature: None,
                })],
                response_model: Some("anthropic/claude-sonnet-4-5".to_string()),
                response_id: Some("chatcmpl_123".to_string()),
                usage: Usage {
                    input: 1,
                    output: 2,
                    total_tokens: 3,
                    ..Usage::default()
                },
                stop_reason: AssistantStopReason::Stop,
                error_message: None,
                diagnostics: vec![create_message_diagnostic(
                    "warning",
                    "upstream warning",
                    None,
                )],
                timestamp_millis: 123,
            },
        }])
        .expect("stream");

        let message = stream.result().expect("final result");
        assert_eq!(
            message.response_model.as_deref(),
            Some("anthropic/claude-sonnet-4-5")
        );
        assert_eq!(message.response_id.as_deref(), Some("chatcmpl_123"));
        assert_eq!(message.diagnostics.len(), 1);
        assert_eq!(message.usage.total_tokens, 3);
    }
}
