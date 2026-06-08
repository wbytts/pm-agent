use std::collections::VecDeque;

use crate::{AssistantMessage, AssistantMessageEvent};

pub struct EventStream<T, R> {
    queue: VecDeque<T>,
    done: bool,
    final_result: Option<R>,
    is_complete: Box<dyn Fn(&T) -> bool + Send + Sync>,
    extract_result: Box<dyn Fn(&T) -> R + Send + Sync>,
}

impl<T, R> EventStream<T, R> {
    pub fn new(
        is_complete: impl Fn(&T) -> bool + Send + Sync + 'static,
        extract_result: impl Fn(&T) -> R + Send + Sync + 'static,
    ) -> Self {
        Self {
            queue: VecDeque::new(),
            done: false,
            final_result: None,
            is_complete: Box::new(is_complete),
            extract_result: Box::new(extract_result),
        }
    }

    pub fn push(&mut self, event: T) {
        if self.done {
            return;
        }
        if (self.is_complete)(&event) {
            self.done = true;
            self.final_result = Some((self.extract_result)(&event));
        }
        self.queue.push_back(event);
    }

    pub fn end(&mut self, result: Option<R>) {
        self.done = true;
        if result.is_some() {
            self.final_result = result;
        }
    }

    pub fn result(&self) -> Option<&R> {
        self.final_result.as_ref()
    }

    pub fn into_result(self) -> Option<R> {
        self.final_result
    }

    pub fn is_done(&self) -> bool {
        self.done
    }

    pub fn len(&self) -> usize {
        self.queue.len()
    }

    pub fn is_empty(&self) -> bool {
        self.queue.is_empty()
    }
}

impl<T, R> Iterator for EventStream<T, R> {
    type Item = T;

    fn next(&mut self) -> Option<Self::Item> {
        self.queue.pop_front()
    }
}

pub type AssistantMessageEventStream = EventStream<AssistantMessageEvent, AssistantMessage>;

pub fn create_assistant_message_event_stream() -> AssistantMessageEventStream {
    AssistantMessageEventStream::new(
        |event| {
            matches!(
                event,
                AssistantMessageEvent::Done { .. } | AssistantMessageEvent::Error { .. }
            )
        },
        |event| match event {
            AssistantMessageEvent::Done { message } => message.clone(),
            AssistantMessageEvent::Error { error } => error.clone(),
            _ => unreachable!("assistant stream result requested before completion"),
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{MessageRole, Usage};

    #[test]
    fn event_stream_queues_events_and_captures_final_result() {
        let mut stream = create_assistant_message_event_stream();
        stream.push(AssistantMessageEvent::TextDelta {
            text: "a".to_string(),
        });
        let message = AssistantMessage::from_text("done", Usage::default());
        stream.push(AssistantMessageEvent::Done {
            message: message.clone(),
        });
        stream.push(AssistantMessageEvent::TextDelta {
            text: "ignored".to_string(),
        });

        assert!(stream.is_done());
        assert_eq!(stream.result(), Some(&message));
        assert_eq!(
            stream
                .map(|event| match event {
                    AssistantMessageEvent::TextDelta { text } => text,
                    AssistantMessageEvent::ThinkingStart { .. } => "thinking_start".to_string(),
                    AssistantMessageEvent::ThinkingDelta { .. } => "thinking_delta".to_string(),
                    AssistantMessageEvent::ThinkingEnd { .. } => "thinking_end".to_string(),
                    AssistantMessageEvent::ToolCallStart { .. } => "tool_start".to_string(),
                    AssistantMessageEvent::ToolCallDelta { .. } => "tool_delta".to_string(),
                    AssistantMessageEvent::ToolCallEnd { .. } => "tool_end".to_string(),
                    AssistantMessageEvent::Done { message } => message.content,
                    AssistantMessageEvent::Usage { .. } => "usage".to_string(),
                    AssistantMessageEvent::Error { error } =>
                        error.error_message.unwrap_or_default(),
                })
                .collect::<Vec<_>>(),
            vec!["a".to_string(), "done".to_string()]
        );
    }

    #[test]
    fn error_event_becomes_final_result() {
        let mut stream = create_assistant_message_event_stream();
        stream.push(AssistantMessageEvent::Error {
            error: AssistantMessage {
                role: MessageRole::Assistant,
                content: String::new(),
                content_blocks: Vec::new(),
                usage: Usage::default(),
                stop_reason: crate::AssistantStopReason::Error,
                error_message: Some("failed".to_string()),
                diagnostics: Vec::new(),
            },
        });
        assert_eq!(
            stream
                .result()
                .and_then(|message| message.error_message.as_deref()),
            Some("failed")
        );
    }
}
