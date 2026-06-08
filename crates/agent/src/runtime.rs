use ai::{
    AssistantContentBlock, LanguageModelProvider, Message as AiMessage, MessageRole,
    RichAssistantMessage, StreamEvent, StreamRequest, TextContent, ThinkingContent, ToolCall,
};
use serde_json::json;
use std::collections::BTreeMap;

use crate::error::{AgentError, AgentResult};
use crate::state::{AgentEvent, AgentMessage, AgentState};

pub struct Agent<P: LanguageModelProvider> {
    pub(crate) state: AgentState,
    provider: P,
}

impl<P: LanguageModelProvider> Agent<P> {
    pub fn new(state: AgentState, provider: P) -> Self {
        Self { state, provider }
    }

    pub fn state(&self) -> &AgentState {
        &self.state
    }

    pub fn prompt(&mut self, prompt: impl Into<String>) -> AgentResult<Vec<AgentEvent>> {
        let prompt = prompt.into();
        if prompt.trim().is_empty() {
            return Err(AgentError::EmptyPrompt);
        }

        self.state.is_streaming = true;
        self.state
            .messages
            .push(AgentMessage::new(MessageRole::User, prompt));

        let mut request_messages = Vec::new();
        if !self.state.system_prompt.trim().is_empty() {
            request_messages.push(AiMessage {
                role: MessageRole::System,
                content: self.state.system_prompt.clone(),
            });
        }
        request_messages.extend(self.state.messages.iter().map(|message| AiMessage {
            role: message.role.clone(),
            content: message.content.clone(),
        }));

        let metadata = BTreeMap::from([("sessionId".to_string(), json!(self.state.session_id))]);
        let mut events = vec![AgentEvent::AgentStart];
        let response = self
            .provider
            .stream(StreamRequest {
                model: self.state.model.clone(),
                messages: request_messages,
                rich_messages: Vec::new(),
                tools: Vec::new(),
                metadata,
            })
            .map_err(|error| AgentError::Ai(error.to_string()))?;

        let mut assistant_content_blocks = Vec::new();
        let mut assistant_usage = None;
        for event in response {
            match event {
                StreamEvent::TextDelta { text } => {
                    append_text_block(&mut assistant_content_blocks, 0, &text);
                    events.push(AgentEvent::MessageDelta { text });
                }
                StreamEvent::ThinkingStart { content_index } => {
                    set_content_block(
                        &mut assistant_content_blocks,
                        content_index,
                        AssistantContentBlock::Thinking(ThinkingContent {
                            thinking: String::new(),
                            thinking_signature: None,
                            redacted: false,
                        }),
                    );
                }
                StreamEvent::ThinkingDelta {
                    content_index,
                    delta,
                } => {
                    append_thinking_block(&mut assistant_content_blocks, content_index, &delta);
                }
                StreamEvent::ThinkingEnd {
                    content_index,
                    content,
                    thinking_signature,
                    redacted,
                } => {
                    set_content_block(
                        &mut assistant_content_blocks,
                        content_index,
                        AssistantContentBlock::Thinking(ThinkingContent {
                            thinking: content,
                            thinking_signature,
                            redacted,
                        }),
                    );
                }
                StreamEvent::ToolCallStart { content_index } => {
                    set_content_block(
                        &mut assistant_content_blocks,
                        content_index,
                        AssistantContentBlock::ToolCall(ToolCall {
                            id: String::new(),
                            name: String::new(),
                            arguments: Default::default(),
                            thought_signature: None,
                        }),
                    );
                }
                StreamEvent::ToolCallDelta { .. } => {}
                StreamEvent::ToolCallEnd {
                    content_index,
                    tool_call,
                } => {
                    set_content_block(
                        &mut assistant_content_blocks,
                        content_index,
                        AssistantContentBlock::ToolCall(ToolCall {
                            id: tool_call.id,
                            name: tool_call.name,
                            arguments: tool_call.arguments,
                            thought_signature: tool_call.thought_signature,
                        }),
                    );
                }
                StreamEvent::Usage { usage } => {
                    assistant_usage = Some(usage.clone());
                    events.push(AgentEvent::Usage { usage });
                }
                StreamEvent::Finished { message } => {
                    if assistant_content_blocks.is_empty() && !message.content.is_empty() {
                        assistant_content_blocks.push(AssistantContentBlock::Text(TextContent {
                            text: message.content.clone(),
                            text_signature: None,
                        }));
                    }
                    let message = AgentMessage {
                        role: message.role,
                        content: message.content,
                        content_blocks: assistant_content_blocks.clone(),
                        user_content_blocks: Vec::new(),
                        tool_call_id: None,
                        tool_name: None,
                        details: None,
                        is_error: false,
                        usage: assistant_usage.clone(),
                        stop_reason: Some(ai::AssistantStopReason::Stop),
                    };
                    self.state.messages.push(message.clone());
                    events.push(AgentEvent::MessageEnd { message });
                }
                StreamEvent::RichFinished { message } => {
                    let content = rich_assistant_text(&message);
                    if assistant_content_blocks.is_empty() {
                        assistant_content_blocks = message.content.clone();
                    }
                    let message = AgentMessage {
                        role: MessageRole::Assistant,
                        content,
                        content_blocks: assistant_content_blocks.clone(),
                        user_content_blocks: Vec::new(),
                        tool_call_id: None,
                        tool_name: None,
                        details: None,
                        is_error: false,
                        usage: assistant_usage.clone().or_else(|| {
                            (message.usage != Default::default()).then_some(message.usage)
                        }),
                        stop_reason: Some(message.stop_reason),
                    };
                    self.state.messages.push(message.clone());
                    events.push(AgentEvent::MessageEnd { message });
                }
                StreamEvent::Error { message } => events.push(AgentEvent::Error { message }),
            }
        }

        self.state.is_streaming = false;
        events.push(AgentEvent::AgentEnd {
            messages: self.state.messages.clone(),
        });
        Ok(events)
    }
}

fn rich_assistant_text(message: &RichAssistantMessage) -> String {
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

fn set_content_block(
    content_blocks: &mut Vec<AssistantContentBlock>,
    content_index: usize,
    block: AssistantContentBlock,
) {
    if content_blocks.len() <= content_index {
        content_blocks.resize_with(content_index + 1, || {
            AssistantContentBlock::Text(TextContent {
                text: String::new(),
                text_signature: None,
            })
        });
    }
    content_blocks[content_index] = block;
}

fn append_text_block(
    content_blocks: &mut Vec<AssistantContentBlock>,
    content_index: usize,
    delta: &str,
) {
    if content_blocks.len() <= content_index {
        set_content_block(
            content_blocks,
            content_index,
            AssistantContentBlock::Text(TextContent {
                text: String::new(),
                text_signature: None,
            }),
        );
    }
    match &mut content_blocks[content_index] {
        AssistantContentBlock::Text(text) => text.text.push_str(delta),
        _ => set_content_block(
            content_blocks,
            content_index,
            AssistantContentBlock::Text(TextContent {
                text: delta.to_string(),
                text_signature: None,
            }),
        ),
    }
}

fn append_thinking_block(
    content_blocks: &mut Vec<AssistantContentBlock>,
    content_index: usize,
    delta: &str,
) {
    if content_blocks.len() <= content_index {
        set_content_block(
            content_blocks,
            content_index,
            AssistantContentBlock::Thinking(ThinkingContent {
                thinking: String::new(),
                thinking_signature: None,
                redacted: false,
            }),
        );
    }
    match &mut content_blocks[content_index] {
        AssistantContentBlock::Thinking(thinking) => thinking.thinking.push_str(delta),
        _ => set_content_block(
            content_blocks,
            content_index,
            AssistantContentBlock::Thinking(ThinkingContent {
                thinking: delta.to_string(),
                thinking_signature: None,
                redacted: false,
            }),
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ai::{types::StreamToolCall, AiResult, Model, ToolCall, Usage};
    use serde_json::json;
    use std::collections::BTreeMap;
    use std::sync::{Arc, Mutex};

    #[derive(Debug, Clone)]
    struct ToolCallProvider;

    impl LanguageModelProvider for ToolCallProvider {
        fn stream(&self, _request: StreamRequest) -> AiResult<Vec<StreamEvent>> {
            Ok(vec![
                StreamEvent::TextDelta {
                    text: "checking".to_string(),
                },
                StreamEvent::ToolCallStart { content_index: 1 },
                StreamEvent::ToolCallDelta {
                    content_index: 1,
                    delta: r#"{"path":"README.md"}"#.to_string(),
                },
                StreamEvent::ToolCallEnd {
                    content_index: 1,
                    tool_call: StreamToolCall {
                        id: "toolu_1".to_string(),
                        name: "read".to_string(),
                        arguments: BTreeMap::from([("path".to_string(), json!("README.md"))]),
                        thought_signature: None,
                    },
                },
                StreamEvent::Usage {
                    usage: Usage {
                        input: 1,
                        output: 2,
                        total_tokens: 3,
                        ..Usage::default()
                    },
                },
                StreamEvent::Finished {
                    message: AiMessage {
                        role: MessageRole::Assistant,
                        content: "checking".to_string(),
                    },
                },
            ])
        }
    }

    #[test]
    fn runtime_preserves_assistant_tool_call_blocks_for_agent_loop() {
        let state = AgentState {
            session_id: "session".to_string(),
            system_prompt: String::new(),
            model: Model {
                id: "model".to_string(),
                provider: "local".to_string(),
                api: "faux".to_string(),
                display_name: "Model".to_string(),
                context_window: 1000,
                ..Model::default()
            },
            messages: Vec::new(),
            is_streaming: false,
        };
        let mut agent = Agent::new(state, ToolCallProvider);

        agent.prompt("inspect").expect("prompt should run");

        let assistant = agent
            .state()
            .messages
            .last()
            .expect("assistant message should be stored");
        assert_eq!(assistant.content, "checking");
        assert_eq!(assistant.usage.as_ref().expect("usage").total_tokens, 3);
        assert!(matches!(
            assistant.content_blocks.as_slice(),
            [
                ai::AssistantContentBlock::Text(text),
                ai::AssistantContentBlock::ToolCall(ToolCall { id, name, arguments, .. })
            ] if text.text == "checking"
                && id == "toolu_1"
                && name == "read"
                && arguments["path"] == json!("README.md")
        ));
    }

    #[derive(Debug, Clone)]
    struct CapturingProvider {
        metadata: Arc<Mutex<Option<BTreeMap<String, serde_json::Value>>>>,
    }

    impl LanguageModelProvider for CapturingProvider {
        fn stream(&self, request: StreamRequest) -> AiResult<Vec<StreamEvent>> {
            *self.metadata.lock().expect("metadata lock") = Some(request.metadata);
            Ok(vec![StreamEvent::Finished {
                message: AiMessage {
                    role: MessageRole::Assistant,
                    content: "ok".to_string(),
                },
            }])
        }
    }

    #[test]
    fn runtime_forwards_session_id_to_stream_metadata_like_pi_agent() {
        let metadata = Arc::new(Mutex::new(None));
        let state = AgentState {
            session_id: "session-abc".to_string(),
            system_prompt: String::new(),
            model: Model {
                id: "model".to_string(),
                provider: "local".to_string(),
                api: "faux".to_string(),
                display_name: "Model".to_string(),
                context_window: 1000,
                ..Model::default()
            },
            messages: Vec::new(),
            is_streaming: false,
        };
        let mut agent = Agent::new(
            state,
            CapturingProvider {
                metadata: metadata.clone(),
            },
        );

        agent.prompt("hello").expect("prompt should run");

        let captured = metadata
            .lock()
            .expect("metadata lock")
            .clone()
            .expect("provider should receive request metadata");
        assert_eq!(captured.get("sessionId"), Some(&json!("session-abc")));
    }
}
