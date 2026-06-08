use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::event_stream::EventStream;
use crate::types::StreamToolCall;
use crate::{
    parse_streaming_json, AssistantContentBlock, AssistantMessage, AssistantStopReason, Message,
    MessageRole, Model, ModelThinkingLevel, TextContent, ThinkingContent, ToolCall, ToolDefinition,
    Usage,
};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ProxyContext {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub system_prompt: Option<String>,
    pub messages: Vec<Message>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tools: Vec<ToolDefinition>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ProxySerializableStreamOptions {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_tokens: Option<usize>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reasoning: Option<ModelThinkingLevel>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cache_retention: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub headers: BTreeMap<String, String>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub metadata: BTreeMap<String, Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub transport: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub thinking_budgets: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_retry_delay_ms: Option<u64>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ProxyStreamOptions {
    pub auth_token: String,
    pub proxy_url: String,
    pub timestamp_millis: u128,
    pub stream_options: ProxySerializableStreamOptions,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ProxyStreamRequestBody {
    model: Model,
    context: ProxyContext,
    options: ProxySerializableStreamOptions,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(
    tag = "type",
    rename_all = "snake_case",
    rename_all_fields = "camelCase"
)]
pub enum ProxyAssistantMessageEvent {
    Start,
    TextStart {
        content_index: usize,
    },
    TextDelta {
        content_index: usize,
        delta: String,
    },
    TextEnd {
        content_index: usize,
        #[serde(default)]
        content_signature: Option<String>,
    },
    ThinkingStart {
        content_index: usize,
    },
    ThinkingDelta {
        content_index: usize,
        delta: String,
    },
    ThinkingEnd {
        content_index: usize,
        #[serde(default)]
        content_signature: Option<String>,
    },
    #[serde(rename = "toolcall_start")]
    ToolCallStart {
        content_index: usize,
        id: String,
        tool_name: String,
    },
    #[serde(rename = "toolcall_delta")]
    ToolCallDelta {
        content_index: usize,
        delta: String,
    },
    #[serde(rename = "toolcall_end")]
    ToolCallEnd {
        content_index: usize,
    },
    Done {
        reason: AssistantStopReason,
        usage: Usage,
    },
    Error {
        reason: AssistantStopReason,
        #[serde(default)]
        error_message: Option<String>,
        usage: Usage,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub enum ProxyAssistantMessageEventOutput {
    Start,
    TextStart {
        content_index: usize,
    },
    TextDelta {
        content_index: usize,
        delta: String,
    },
    TextEnd {
        content_index: usize,
        content: String,
    },
    ThinkingStart {
        content_index: usize,
    },
    ThinkingDelta {
        content_index: usize,
        delta: String,
    },
    ThinkingEnd {
        content_index: usize,
        content: String,
    },
    ToolCallStart {
        content_index: usize,
    },
    ToolCallDelta {
        content_index: usize,
        delta: String,
    },
    ToolCallEnd {
        content_index: usize,
        tool_call: StreamToolCall,
    },
    Done {
        reason: AssistantStopReason,
        message: AssistantMessage,
    },
    Error {
        reason: AssistantStopReason,
        error: AssistantMessage,
    },
}

pub type ProxyAssistantMessageEventStream =
    EventStream<ProxyAssistantMessageEventOutput, AssistantMessage>;

pub fn create_proxy_message_event_stream() -> ProxyAssistantMessageEventStream {
    EventStream::new(
        |event| {
            matches!(
                event,
                ProxyAssistantMessageEventOutput::Done { .. }
                    | ProxyAssistantMessageEventOutput::Error { .. }
            )
        },
        |event| match event {
            ProxyAssistantMessageEventOutput::Done { message, .. } => message.clone(),
            ProxyAssistantMessageEventOutput::Error { error, .. } => error.clone(),
            _ => unreachable!("proxy stream result requested before completion"),
        },
    )
}

#[derive(Debug, Clone)]
pub struct ProxyMessageState {
    partial: AssistantMessage,
    tool_call_json: BTreeMap<usize, String>,
    timestamp_millis: u128,
}

impl ProxyMessageState {
    pub fn new(model: &Model, timestamp_millis: u128) -> Self {
        let mut partial = AssistantMessage::from_text(String::new(), Usage::default());
        partial.diagnostics = vec![
            crate::create_message_diagnostic("api", model.api.clone(), None),
            crate::create_message_diagnostic("provider", model.provider.clone(), None),
            crate::create_message_diagnostic("model", model.id.clone(), None),
            crate::create_message_diagnostic("timestampMillis", timestamp_millis.to_string(), None),
        ];
        Self {
            partial,
            tool_call_json: BTreeMap::new(),
            timestamp_millis,
        }
    }

    pub fn partial(&self) -> &AssistantMessage {
        &self.partial
    }

    pub fn timestamp_millis(&self) -> u128 {
        self.timestamp_millis
    }

    pub fn process(
        &mut self,
        proxy_event: ProxyAssistantMessageEvent,
    ) -> Result<Option<ProxyAssistantMessageEventOutput>, String> {
        match proxy_event {
            ProxyAssistantMessageEvent::Start => Ok(Some(ProxyAssistantMessageEventOutput::Start)),
            ProxyAssistantMessageEvent::TextStart { content_index } => {
                self.set_block(
                    content_index,
                    AssistantContentBlock::Text(TextContent {
                        text: String::new(),
                        text_signature: None,
                    }),
                );
                Ok(Some(ProxyAssistantMessageEventOutput::TextStart {
                    content_index,
                }))
            }
            ProxyAssistantMessageEvent::TextDelta {
                content_index,
                delta,
            } => {
                let Some(AssistantContentBlock::Text(text)) =
                    self.partial.content_blocks.get_mut(content_index)
                else {
                    return Err("Received text_delta for non-text content".to_string());
                };
                text.text.push_str(&delta);
                self.sync_text_content();
                Ok(Some(ProxyAssistantMessageEventOutput::TextDelta {
                    content_index,
                    delta,
                }))
            }
            ProxyAssistantMessageEvent::TextEnd {
                content_index,
                content_signature,
            } => {
                let Some(AssistantContentBlock::Text(text)) =
                    self.partial.content_blocks.get_mut(content_index)
                else {
                    return Err("Received text_end for non-text content".to_string());
                };
                text.text_signature = content_signature;
                let content = text.text.clone();
                Ok(Some(ProxyAssistantMessageEventOutput::TextEnd {
                    content_index,
                    content,
                }))
            }
            ProxyAssistantMessageEvent::ThinkingStart { content_index } => {
                self.set_block(
                    content_index,
                    AssistantContentBlock::Thinking(ThinkingContent {
                        thinking: String::new(),
                        thinking_signature: None,
                        redacted: false,
                    }),
                );
                Ok(Some(ProxyAssistantMessageEventOutput::ThinkingStart {
                    content_index,
                }))
            }
            ProxyAssistantMessageEvent::ThinkingDelta {
                content_index,
                delta,
            } => {
                let Some(AssistantContentBlock::Thinking(thinking)) =
                    self.partial.content_blocks.get_mut(content_index)
                else {
                    return Err("Received thinking_delta for non-thinking content".to_string());
                };
                thinking.thinking.push_str(&delta);
                Ok(Some(ProxyAssistantMessageEventOutput::ThinkingDelta {
                    content_index,
                    delta,
                }))
            }
            ProxyAssistantMessageEvent::ThinkingEnd {
                content_index,
                content_signature,
            } => {
                let Some(AssistantContentBlock::Thinking(thinking)) =
                    self.partial.content_blocks.get_mut(content_index)
                else {
                    return Err("Received thinking_end for non-thinking content".to_string());
                };
                thinking.thinking_signature = content_signature;
                let content = thinking.thinking.clone();
                Ok(Some(ProxyAssistantMessageEventOutput::ThinkingEnd {
                    content_index,
                    content,
                }))
            }
            ProxyAssistantMessageEvent::ToolCallStart {
                content_index,
                id,
                tool_name,
            } => {
                self.tool_call_json.insert(content_index, String::new());
                self.set_block(
                    content_index,
                    AssistantContentBlock::ToolCall(ToolCall {
                        id,
                        name: tool_name,
                        arguments: BTreeMap::new(),
                        thought_signature: None,
                    }),
                );
                Ok(Some(ProxyAssistantMessageEventOutput::ToolCallStart {
                    content_index,
                }))
            }
            ProxyAssistantMessageEvent::ToolCallDelta {
                content_index,
                delta,
            } => {
                let Some(AssistantContentBlock::ToolCall(tool_call)) =
                    self.partial.content_blocks.get_mut(content_index)
                else {
                    return Err("Received toolcall_delta for non-toolCall content".to_string());
                };
                let partial_json = self.tool_call_json.entry(content_index).or_default();
                partial_json.push_str(&delta);
                let arguments = parse_streaming_json(Some(partial_json.as_str()));
                tool_call.arguments = json_object_to_map(arguments);
                Ok(Some(ProxyAssistantMessageEventOutput::ToolCallDelta {
                    content_index,
                    delta,
                }))
            }
            ProxyAssistantMessageEvent::ToolCallEnd { content_index } => {
                let Some(AssistantContentBlock::ToolCall(tool_call)) =
                    self.partial.content_blocks.get(content_index)
                else {
                    return Ok(None);
                };
                self.tool_call_json.remove(&content_index);
                Ok(Some(ProxyAssistantMessageEventOutput::ToolCallEnd {
                    content_index,
                    tool_call: StreamToolCall {
                        id: tool_call.id.clone(),
                        name: tool_call.name.clone(),
                        arguments: tool_call.arguments.clone(),
                        thought_signature: tool_call.thought_signature.clone(),
                    },
                }))
            }
            ProxyAssistantMessageEvent::Done { reason, usage } => {
                self.partial.stop_reason = reason.clone();
                self.partial.usage = usage;
                Ok(Some(ProxyAssistantMessageEventOutput::Done {
                    reason,
                    message: self.partial.clone(),
                }))
            }
            ProxyAssistantMessageEvent::Error {
                reason,
                error_message,
                usage,
            } => {
                self.partial.stop_reason = reason.clone();
                self.partial.error_message = error_message;
                self.partial.usage = usage;
                Ok(Some(ProxyAssistantMessageEventOutput::Error {
                    reason,
                    error: self.partial.clone(),
                }))
            }
        }
    }

    fn set_block(&mut self, content_index: usize, block: AssistantContentBlock) {
        if self.partial.content_blocks.len() <= content_index {
            self.partial
                .content_blocks
                .resize_with(content_index + 1, || {
                    AssistantContentBlock::Text(TextContent {
                        text: String::new(),
                        text_signature: None,
                    })
                });
        }
        self.partial.content_blocks[content_index] = block;
        self.sync_text_content();
    }

    fn sync_text_content(&mut self) {
        self.partial.content = self
            .partial
            .content_blocks
            .iter()
            .filter_map(|block| match block {
                AssistantContentBlock::Text(text) => Some(text.text.as_str()),
                _ => None,
            })
            .collect::<String>();
        self.partial.role = MessageRole::Assistant;
    }
}

fn json_object_to_map(value: serde_json::Value) -> BTreeMap<String, serde_json::Value> {
    match value {
        serde_json::Value::Object(object) => object.into_iter().collect(),
        _ => BTreeMap::new(),
    }
}

pub fn stream_proxy(
    model: Model,
    context: ProxyContext,
    options: ProxyStreamOptions,
) -> Result<Vec<ProxyAssistantMessageEventOutput>, String> {
    let stream = stream_proxy_event_stream(model, context, options)?;
    Ok(stream.collect())
}

pub fn stream_proxy_event_stream(
    model: Model,
    context: ProxyContext,
    options: ProxyStreamOptions,
) -> Result<ProxyAssistantMessageEventStream, String> {
    let timestamp_millis = options.timestamp_millis;
    let url = format!("{}/api/stream", options.proxy_url.trim_end_matches('/'));
    let body = ProxyStreamRequestBody {
        model: model.clone(),
        context,
        options: options.stream_options,
    };
    let response = match reqwest::blocking::Client::new()
        .post(url)
        .bearer_auth(&options.auth_token)
        .json(&body)
        .send()
    {
        Ok(response) => response,
        Err(error) => {
            return Ok(proxy_error_event_stream(
                &model,
                timestamp_millis,
                AssistantStopReason::Error,
                format!("Proxy request failed: {error}"),
            ));
        }
    };

    if !response.status().is_success() {
        let status = response.status();
        let status_text = status
            .canonical_reason()
            .map(|reason| format!(" {reason}"))
            .unwrap_or_default();
        let error_message = response
            .json::<serde_json::Value>()
            .ok()
            .and_then(|value| {
                value
                    .get("error")
                    .and_then(serde_json::Value::as_str)
                    .map(|error| format!("Proxy error: {error}"))
            })
            .unwrap_or_else(|| format!("Proxy error: {status}{status_text}"));
        return Ok(proxy_error_event_stream(
            &model,
            timestamp_millis,
            AssistantStopReason::Error,
            error_message,
        ));
    }

    let text = match response.text() {
        Ok(text) => text,
        Err(error) => {
            return Ok(proxy_error_event_stream(
                &model,
                timestamp_millis,
                AssistantStopReason::Error,
                format!("Proxy response read failed: {error}"),
            ));
        }
    };
    process_proxy_sse_text_stream(&model, timestamp_millis, &text).or_else(|error| {
        Ok(proxy_error_event_stream(
            &model,
            timestamp_millis,
            AssistantStopReason::Error,
            error,
        ))
    })
}

pub fn process_proxy_sse_text(
    model: &Model,
    timestamp_millis: u128,
    text: &str,
) -> Result<Vec<ProxyAssistantMessageEventOutput>, String> {
    let stream = process_proxy_sse_text_stream(model, timestamp_millis, text)?;
    Ok(stream.collect())
}

pub fn process_proxy_sse_text_stream(
    model: &Model,
    timestamp_millis: u128,
    text: &str,
) -> Result<ProxyAssistantMessageEventStream, String> {
    let mut state = ProxyMessageState::new(model, timestamp_millis);
    let mut stream = create_proxy_message_event_stream();
    for line in text.lines() {
        let line = line.trim_end_matches('\r');
        let Some(data) = line.strip_prefix("data: ") else {
            continue;
        };
        let data = data.trim();
        if data.is_empty() {
            continue;
        }
        let event = serde_json::from_str::<ProxyAssistantMessageEvent>(data)
            .map_err(|error| format!("Invalid proxy event JSON: {error}"))?;
        if let Some(output) = state.process(event)? {
            stream.push(output);
        }
    }
    Ok(stream)
}

fn proxy_error_event_stream(
    model: &Model,
    timestamp_millis: u128,
    reason: AssistantStopReason,
    error_message: String,
) -> ProxyAssistantMessageEventStream {
    let mut state = ProxyMessageState::new(model, timestamp_millis);
    let output = state
        .process(ProxyAssistantMessageEvent::Error {
            reason,
            error_message: Some(error_message),
            usage: Usage::default(),
        })
        .expect("proxy error event is always valid")
        .expect("proxy error event emits output");
    let mut stream = create_proxy_message_event_stream();
    stream.push(output);
    stream
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Message, TextContent, ThinkingContent, ToolCall, ToolDefinition};
    use serde_json::json;
    use std::collections::BTreeMap;
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::thread;

    #[test]
    fn proxy_events_reconstruct_partial_text_thinking_tool_calls_and_done() {
        let model = Model {
            id: "claude-test".to_string(),
            provider: "anthropic".to_string(),
            api: "messages".to_string(),
            ..Model::default()
        };
        let usage = Usage {
            input: 3,
            output: 5,
            total_tokens: 8,
            ..Usage::default()
        };
        let mut state = ProxyMessageState::new(&model, 42);

        assert!(matches!(
            state
                .process(ProxyAssistantMessageEvent::Start)
                .expect("start"),
            Some(ProxyAssistantMessageEventOutput::Start)
        ));
        state
            .process(ProxyAssistantMessageEvent::TextStart { content_index: 0 })
            .expect("text start");
        state
            .process(ProxyAssistantMessageEvent::TextDelta {
                content_index: 0,
                delta: "hel".to_string(),
            })
            .expect("text delta");
        state
            .process(ProxyAssistantMessageEvent::TextEnd {
                content_index: 0,
                content_signature: Some("sig-text".to_string()),
            })
            .expect("text end");
        state
            .process(ProxyAssistantMessageEvent::ThinkingStart { content_index: 1 })
            .expect("thinking start");
        state
            .process(ProxyAssistantMessageEvent::ThinkingDelta {
                content_index: 1,
                delta: "plan".to_string(),
            })
            .expect("thinking delta");
        state
            .process(ProxyAssistantMessageEvent::ThinkingEnd {
                content_index: 1,
                content_signature: Some("sig-thinking".to_string()),
            })
            .expect("thinking end");
        state
            .process(ProxyAssistantMessageEvent::ToolCallStart {
                content_index: 2,
                id: "toolu_1".to_string(),
                tool_name: "edit".to_string(),
            })
            .expect("tool start");
        state
            .process(ProxyAssistantMessageEvent::ToolCallDelta {
                content_index: 2,
                delta: r#"{"path":"README.md""#.to_string(),
            })
            .expect("tool delta");
        let output = state
            .process(ProxyAssistantMessageEvent::ToolCallDelta {
                content_index: 2,
                delta: r#","limit":5}"#.to_string(),
            })
            .expect("tool delta")
            .expect("output");
        assert!(matches!(
            output,
            ProxyAssistantMessageEventOutput::ToolCallDelta {
                content_index: 2,
                ..
            }
        ));
        let output = state
            .process(ProxyAssistantMessageEvent::ToolCallEnd { content_index: 2 })
            .expect("tool end")
            .expect("output");
        assert!(matches!(
            output,
            ProxyAssistantMessageEventOutput::ToolCallEnd {
                content_index: 2,
                tool_call: StreamToolCall {
                    id,
                    name,
                    arguments,
                    ..
                }
            } if id == "toolu_1" && name == "edit" && arguments["path"] == json!("README.md")
                && arguments["limit"] == json!(5)
        ));

        let output = state
            .process(ProxyAssistantMessageEvent::Done {
                reason: AssistantStopReason::ToolUse,
                usage: usage.clone(),
            })
            .expect("done")
            .expect("output");

        let ProxyAssistantMessageEventOutput::Done { message, reason } = output else {
            panic!("expected done");
        };
        assert_eq!(reason, AssistantStopReason::ToolUse);
        assert_eq!(message.role, crate::MessageRole::Assistant);
        assert_eq!(message.usage, usage);
        assert_eq!(message.content, "hel");
        assert_eq!(
            message.content_blocks,
            vec![
                AssistantContentBlock::Text(TextContent {
                    text: "hel".to_string(),
                    text_signature: Some("sig-text".to_string()),
                }),
                AssistantContentBlock::Thinking(ThinkingContent {
                    thinking: "plan".to_string(),
                    thinking_signature: Some("sig-thinking".to_string()),
                    redacted: false,
                }),
                AssistantContentBlock::ToolCall(ToolCall {
                    id: "toolu_1".to_string(),
                    name: "edit".to_string(),
                    arguments: BTreeMap::from([
                        ("limit".to_string(), json!(5)),
                        ("path".to_string(), json!("README.md")),
                    ]),
                    thought_signature: None,
                }),
            ]
        );
    }

    #[test]
    fn proxy_events_report_error_and_reject_mismatched_delta_types() {
        let model = Model::default();
        let mut state = ProxyMessageState::new(&model, 7);

        state
            .process(ProxyAssistantMessageEvent::TextStart { content_index: 0 })
            .expect("text start");
        let error = state
            .process(ProxyAssistantMessageEvent::ThinkingDelta {
                content_index: 0,
                delta: "bad".to_string(),
            })
            .expect_err("mismatched delta should fail");
        assert_eq!(error, "Received thinking_delta for non-thinking content");

        let output = state
            .process(ProxyAssistantMessageEvent::Error {
                reason: AssistantStopReason::Error,
                error_message: Some("proxy unavailable".to_string()),
                usage: Usage::default(),
            })
            .expect("error")
            .expect("output");
        assert!(matches!(
            output,
            ProxyAssistantMessageEventOutput::Error {
                reason: AssistantStopReason::Error,
                error
            } if error.error_message.as_deref() == Some("proxy unavailable")
        ));
    }

    #[test]
    fn proxy_event_deserializes_pi_wire_names_for_toolcall_events() {
        let event: ProxyAssistantMessageEvent = serde_json::from_value(json!({
            "type": "toolcall_start",
            "contentIndex": 2,
            "id": "toolu_1",
            "toolName": "read"
        }))
        .expect("pi proxy event should deserialize");

        assert_eq!(
            event,
            ProxyAssistantMessageEvent::ToolCallStart {
                content_index: 2,
                id: "toolu_1".to_string(),
                tool_name: "read".to_string()
            }
        );
    }

    #[test]
    fn process_proxy_sse_text_stream_collects_events_and_final_result_like_pi_proxy() {
        let model = Model {
            id: "model".to_string(),
            provider: "local".to_string(),
            api: "faux".to_string(),
            ..Model::default()
        };
        let sse = concat!(
            "data: {\"type\":\"start\"}\n\n",
            "data: {\"type\":\"text_start\",\"contentIndex\":0}\n\n",
            "data: {\"type\":\"text_delta\",\"contentIndex\":0,\"delta\":\"hi\"}\n\n",
            "data: {\"type\":\"text_end\",\"contentIndex\":0,\"contentSignature\":\"sig\"}\n\n",
            "data: {\"type\":\"done\",\"reason\":\"stop\",\"usage\":{\"input\":1,\"output\":2,\"cacheRead\":0,\"cacheWrite\":0,\"totalTokens\":3,\"cost\":{\"input\":0.0,\"output\":0.0,\"cacheRead\":0.0,\"cacheWrite\":0.0,\"total\":0.0}}}\n\n",
        );

        let mut stream = process_proxy_sse_text_stream(&model, 321, sse).expect("stream");

        assert!(stream.is_done());
        assert_eq!(
            stream.result().map(|message| message.content.as_str()),
            Some("hi")
        );
        let events = stream.by_ref().collect::<Vec<_>>();
        assert!(matches!(
            events.first(),
            Some(ProxyAssistantMessageEventOutput::Start)
        ));
        assert!(matches!(
            events.get(1),
            Some(ProxyAssistantMessageEventOutput::TextStart { content_index: 0 })
        ));
        assert!(matches!(
            events.get(3),
            Some(ProxyAssistantMessageEventOutput::TextEnd {
                content_index: 0,
                content,
            }) if content == "hi"
        ));
        assert!(matches!(
            events.last(),
            Some(ProxyAssistantMessageEventOutput::Done {
                reason: AssistantStopReason::Stop,
                message,
            }) if message.usage.total_tokens == 3
        ));
    }

    #[test]
    fn stream_proxy_posts_request_and_processes_sse_events_like_pi_proxy() {
        let listener = TcpListener::bind("127.0.0.1:0").expect("listener");
        let proxy_url = format!("http://{}", listener.local_addr().expect("addr"));
        let server = thread::spawn(move || {
            let (mut stream, _) = listener.accept().expect("accept");
            let request = read_http_request(&mut stream);
            assert!(request.starts_with("POST /api/stream HTTP/1.1"));
            assert!(request
                .to_ascii_lowercase()
                .contains("authorization: bearer secret-token"));
            assert!(request
                .to_ascii_lowercase()
                .contains("content-type: application/json"));

            let body = request.split("\r\n\r\n").nth(1).expect("body");
            let value: serde_json::Value = serde_json::from_str(body).expect("json body");
            assert_eq!(value["model"]["id"], json!("model"));
            assert_eq!(value["context"]["messages"][0]["content"], json!("inspect"));
            assert_eq!(value["context"]["tools"][0]["name"], json!("read"));
            assert_eq!(value["options"]["metadata"]["traceId"], json!("trace-1"));

            let response_body = concat!(
                "data: {\"type\":\"start\"}\n\n",
                "data: {\"type\":\"text_start\",\"contentIndex\":0}\n\n",
                "data: {\"type\":\"text_delta\",\"contentIndex\":0,\"delta\":\"hi\"}\n\n",
                "data: {\"type\":\"done\",\"reason\":\"stop\",\"usage\":{\"input\":1,\"output\":2,\"cacheRead\":0,\"cacheWrite\":0,\"totalTokens\":3,\"cost\":{\"input\":0.0,\"output\":0.0,\"cacheRead\":0.0,\"cacheWrite\":0.0,\"total\":0.0}}}\n\n",
            );
            let response = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                response_body.len(),
                response_body
            );
            stream
                .write_all(response.as_bytes())
                .expect("write response");
        });

        let model = Model {
            id: "model".to_string(),
            provider: "local".to_string(),
            api: "faux".to_string(),
            ..Model::default()
        };
        let outputs = stream_proxy(
            model,
            ProxyContext {
                system_prompt: Some("system".to_string()),
                messages: vec![Message {
                    role: MessageRole::User,
                    content: "inspect".to_string(),
                }],
                tools: vec![ToolDefinition {
                    name: "read".to_string(),
                    description: "Read file".to_string(),
                    parameters: json!({"type": "object"}),
                }],
            },
            ProxyStreamOptions {
                auth_token: "secret-token".to_string(),
                proxy_url,
                timestamp_millis: 123,
                stream_options: ProxySerializableStreamOptions {
                    metadata: BTreeMap::from([("traceId".to_string(), json!("trace-1"))]),
                    ..ProxySerializableStreamOptions::default()
                },
            },
        )
        .expect("stream proxy");
        server.join().expect("server");

        assert!(matches!(
            outputs.first(),
            Some(ProxyAssistantMessageEventOutput::Start)
        ));
        assert!(matches!(
            outputs.get(2),
            Some(ProxyAssistantMessageEventOutput::TextDelta { delta, .. }) if delta == "hi"
        ));
        assert!(matches!(
            outputs.last(),
            Some(ProxyAssistantMessageEventOutput::Done {
                reason: AssistantStopReason::Stop,
            message,
        }) if message.content == "hi" && message.usage.total_tokens == 3
        ));
    }

    #[test]
    fn stream_proxy_event_stream_reports_http_error_as_final_event_like_pi_proxy() {
        let listener = TcpListener::bind("127.0.0.1:0").expect("listener");
        let proxy_url = format!("http://{}", listener.local_addr().expect("addr"));
        let server = thread::spawn(move || {
            let (mut stream, _) = listener.accept().expect("accept");
            let request = read_http_request(&mut stream);
            assert!(request.starts_with("POST /api/stream HTTP/1.1"));

            let response_body = "{\"error\":\"bad token\"}";
            let response = format!(
                "HTTP/1.1 401 Unauthorized\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                response_body.len(),
                response_body
            );
            stream
                .write_all(response.as_bytes())
                .expect("write response");
        });

        let model = Model {
            id: "model".to_string(),
            provider: "local".to_string(),
            api: "faux".to_string(),
            ..Model::default()
        };
        let mut stream = stream_proxy_event_stream(
            model,
            ProxyContext {
                system_prompt: None,
                messages: Vec::new(),
                tools: Vec::new(),
            },
            ProxyStreamOptions {
                auth_token: "bad-token".to_string(),
                proxy_url,
                timestamp_millis: 456,
                stream_options: ProxySerializableStreamOptions::default(),
            },
        )
        .expect("stream");
        server.join().expect("server");

        assert!(stream.is_done());
        assert_eq!(
            stream
                .result()
                .and_then(|message| message.error_message.as_deref()),
            Some("Proxy error: bad token")
        );
        let events = stream.by_ref().collect::<Vec<_>>();
        assert!(matches!(
            events.last(),
            Some(ProxyAssistantMessageEventOutput::Error {
                reason: AssistantStopReason::Error,
                error,
            }) if error.error_message.as_deref() == Some("Proxy error: bad token")
        ));
    }

    fn read_http_request(stream: &mut std::net::TcpStream) -> String {
        let mut buffer = Vec::new();
        let mut chunk = [0; 1024];
        loop {
            let read = stream.read(&mut chunk).expect("read request");
            if read == 0 {
                break;
            }
            buffer.extend_from_slice(&chunk[..read]);
            let request = String::from_utf8_lossy(&buffer);
            if let Some(header_end) = request.find("\r\n\r\n") {
                let headers = &request[..header_end];
                let content_length = headers
                    .lines()
                    .find_map(|line| {
                        line.strip_prefix("content-length: ")
                            .or_else(|| line.strip_prefix("Content-Length: "))
                    })
                    .and_then(|value| value.parse::<usize>().ok())
                    .unwrap_or(0);
                let body_len = buffer.len().saturating_sub(header_end + 4);
                if body_len >= content_length {
                    break;
                }
            }
        }
        String::from_utf8(buffer).expect("utf8 request")
    }
}
