use crate::{
    types::{
        validate_model, AiResult, AssistantMessage, LanguageModelProvider, Message, MessageRole,
        StreamEvent, StreamRequest, Usage,
    },
    ThinkingContent, ToolCall,
};
use serde_json::Value;
use std::collections::{BTreeMap, VecDeque};
use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc, Mutex,
};

static FAUX_TOOL_ID_COUNTER: AtomicUsize = AtomicUsize::new(0);

#[derive(Debug, Clone, Default)]
pub struct EchoProvider;

impl LanguageModelProvider for EchoProvider {
    fn stream(&self, request: StreamRequest) -> AiResult<Vec<StreamEvent>> {
        validate_model(&request.model)?;
        let text = request
            .messages
            .iter()
            .rev()
            .find(|message| message.role == MessageRole::User)
            .map(|message| message.content.clone())
            .unwrap_or_default();

        Ok(vec![
            StreamEvent::TextDelta { text: text.clone() },
            StreamEvent::Finished {
                message: Message {
                    role: MessageRole::Assistant,
                    content: text,
                },
            },
        ])
    }
}

#[derive(Debug, Clone, Default)]
pub struct FauxProvider {
    scripted_responses: Option<Arc<Mutex<VecDeque<String>>>>,
    call_count: Arc<AtomicUsize>,
}

pub fn faux_text(text: impl Into<String>) -> String {
    text.into()
}

pub fn faux_thinking(thinking: impl Into<String>) -> ThinkingContent {
    ThinkingContent {
        thinking: thinking.into(),
        thinking_signature: None,
        redacted: false,
    }
}

pub fn faux_tool_call(name: impl Into<String>, arguments: Value, id: Option<&str>) -> ToolCall {
    ToolCall {
        id: id.map(str::to_string).unwrap_or_else(next_faux_tool_id),
        name: name.into(),
        arguments: match arguments {
            Value::Object(map) => map.into_iter().collect::<BTreeMap<_, _>>(),
            _ => BTreeMap::new(),
        },
        thought_signature: None,
    }
}

pub fn faux_assistant_message(content: impl Into<String>) -> AssistantMessage {
    AssistantMessage::from_text(content, Usage::default())
}

pub fn faux_assistant_error(message: impl Into<String>) -> AssistantMessage {
    AssistantMessage::error(message)
}

fn next_faux_tool_id() -> String {
    let next = FAUX_TOOL_ID_COUNTER.fetch_add(1, Ordering::SeqCst) + 1;
    format!("tool:{next}")
}

impl FauxProvider {
    pub fn with_responses<I, S>(responses: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        Self {
            scripted_responses: Some(Arc::new(Mutex::new(
                responses.into_iter().map(Into::into).collect(),
            ))),
            call_count: Arc::new(AtomicUsize::new(0)),
        }
    }

    pub fn call_count(&self) -> usize {
        self.call_count.load(Ordering::SeqCst)
    }
}

impl LanguageModelProvider for FauxProvider {
    fn stream(&self, request: StreamRequest) -> AiResult<Vec<StreamEvent>> {
        validate_model(&request.model)?;
        self.call_count.fetch_add(1, Ordering::SeqCst);

        if let Some(responses) = &self.scripted_responses {
            let response = responses
                .lock()
                .expect("faux provider responses lock poisoned")
                .pop_front();
            return Ok(match response {
                Some(content) => vec![StreamEvent::Finished {
                    message: Message {
                        role: MessageRole::Assistant,
                        content,
                    },
                }],
                None => vec![StreamEvent::Error {
                    message: "No more faux responses queued".to_string(),
                }],
            });
        }

        let text = request
            .messages
            .iter()
            .rev()
            .find(|message| message.role == MessageRole::User)
            .map(|message| format!("已收到：{}", message.content))
            .unwrap_or_else(|| "已收到。".to_string());

        Ok(vec![StreamEvent::Finished {
            message: Message {
                role: MessageRole::Assistant,
                content: text,
            },
        }])
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{AssistantStopReason, Model, StreamRequest};

    fn faux_request() -> StreamRequest {
        StreamRequest {
            model: Model {
                id: "faux-1".to_string(),
                provider: "faux".to_string(),
                api: "faux".to_string(),
                display_name: "Faux Model".to_string(),
                context_window: 128_000,
                ..Model::default()
            },
            messages: vec![Message {
                role: MessageRole::User,
                content: "hello".to_string(),
            }],
            rich_messages: Vec::new(),
            tools: Vec::new(),
            metadata: Default::default(),
        }
    }

    #[test]
    fn faux_provider_consumes_scripted_responses_and_tracks_call_count() {
        let provider = FauxProvider::with_responses(["first", "second"]);

        let first = provider.stream(faux_request()).expect("first response");
        let second = provider.stream(faux_request()).expect("second response");

        assert_eq!(provider.call_count(), 2);
        assert_finished_content(first, "first");
        assert_finished_content(second, "second");
    }

    #[test]
    fn faux_provider_returns_error_message_when_scripted_queue_is_empty() {
        let provider = FauxProvider::with_responses(Vec::<String>::new());

        let events = provider
            .stream(faux_request())
            .expect("empty queue response");

        assert_eq!(provider.call_count(), 1);
        assert_error_message(events, "No more faux responses queued");
    }

    #[test]
    fn faux_helpers_build_text_and_assistant_messages_like_pi() {
        assert_eq!(faux_text("hello"), "hello");
        assert_eq!(
            faux_thinking("plan"),
            crate::ThinkingContent {
                thinking: "plan".to_string(),
                thinking_signature: None,
                redacted: false,
            }
        );

        let tool_call = faux_tool_call(
            "read_file",
            serde_json::json!({
                "path": "Cargo.toml",
                "limit": 10
            }),
            Some("call-1"),
        );
        assert_eq!(tool_call.id, "call-1");
        assert_eq!(tool_call.name, "read_file");
        assert_eq!(tool_call.arguments["path"], "Cargo.toml");
        assert_eq!(tool_call.arguments["limit"], 10);
        assert_eq!(tool_call.thought_signature, None);

        let generated_tool_call = faux_tool_call("search", serde_json::json!({}), None);
        assert!(generated_tool_call.id.starts_with("tool:"));
        assert_eq!(generated_tool_call.name, "search");

        let message = faux_assistant_message("hello");
        assert_eq!(message.role, MessageRole::Assistant);
        assert_eq!(message.content, "hello");
        assert_eq!(message.stop_reason, AssistantStopReason::Stop);
        assert_eq!(message.usage, Default::default());
        assert_eq!(message.error_message, None);

        let error = faux_assistant_error("boom");
        assert_eq!(error.role, MessageRole::Assistant);
        assert_eq!(error.content, "");
        assert_eq!(error.stop_reason, AssistantStopReason::Error);
        assert_eq!(error.error_message, Some("boom".to_string()));
    }

    fn assert_finished_content(events: Vec<StreamEvent>, expected: &str) {
        match events.as_slice() {
            [StreamEvent::Finished { message }] => {
                assert_eq!(message.role, MessageRole::Assistant);
                assert_eq!(message.content, expected);
            }
            _ => panic!("expected single finished event"),
        }
    }

    fn assert_error_message(events: Vec<StreamEvent>, expected: &str) {
        match events.as_slice() {
            [StreamEvent::Error { message }] => assert_eq!(message, expected),
            _ => panic!("expected single error event"),
        }
    }
}
