use serde::{Deserialize, Serialize};
use std::env;

use crate::conversation::{
    transform_messages, AssistantContentBlock, RichAssistantMessage, RichMessage, TextContent,
    ThinkingContent, ToolCall as RichToolCall, UserContentBlock, UserMessageContent,
};
use crate::providers::{is_cloudflare_provider, resolve_cloudflare_base_url_from_str};
use crate::types::{
    validate_model, AiError, AiResult, AssistantStopReason, LanguageModelProvider, Message,
    MessageRole, Model, ModelThinkingLevel, StreamEvent, StreamRequest, StreamToolCall,
    ToolDefinition,
};
use crate::utils::{parse_json_with_repair, parse_streaming_json, sanitize_surrogates};
use serde_json::{json, Value};

const FINE_GRAINED_TOOL_STREAMING_BETA: &str = "fine-grained-tool-streaming-2025-05-14";

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AnthropicMessagesConfig {
    pub api_key: Option<String>,
    pub base_url: String,
    pub version: String,
    pub max_tokens: u32,
}

#[derive(Debug, Clone)]
pub struct AnthropicMessagesProvider {
    config: AnthropicMessagesConfig,
}

impl AnthropicMessagesProvider {
    pub fn new(config: AnthropicMessagesConfig) -> Self {
        Self { config }
    }

    pub fn from_env() -> Self {
        Self::new(AnthropicMessagesConfig {
            api_key: env::var("ANTHROPIC_API_KEY").ok(),
            base_url: env::var("ANTHROPIC_BASE_URL")
                .unwrap_or_else(|_| "https://api.anthropic.com".to_string()),
            version: env::var("ANTHROPIC_VERSION").unwrap_or_else(|_| "2023-06-01".to_string()),
            max_tokens: env::var("ANTHROPIC_MAX_TOKENS")
                .ok()
                .and_then(|value| value.parse().ok())
                .unwrap_or(4096),
        })
    }

    fn api_key(&self) -> AiResult<String> {
        let key = self.config.api_key.as_deref().unwrap_or_default().trim();
        if key.is_empty() {
            return Err(AiError::MissingApiKey("ANTHROPIC_API_KEY".to_string()));
        }
        Ok(key.to_string())
    }
}

impl LanguageModelProvider for AnthropicMessagesProvider {
    fn stream(&self, request: StreamRequest) -> AiResult<Vec<StreamEvent>> {
        validate_model(&request.model)?;
        let api_key = self.api_key()?;
        let model = request.model.clone();
        let raw_base_url = request
            .model
            .base_url
            .as_deref()
            .unwrap_or(&self.config.base_url);
        let base_url = anthropic_provider_base_url(&request.model.provider, raw_base_url)?;
        let url = format!("{}/v1/messages", base_url.trim_end_matches('/'));
        let beta_header = anthropic_beta_header_for_request(&request);
        let session_affinity_header = anthropic_session_affinity_header(&request);
        let is_oauth_token = anthropic_is_oauth_token(&api_key);
        let payload =
            AnthropicRequest::from_stream_request(request, self.config.max_tokens, is_oauth_token);

        let mut request_builder = reqwest::blocking::Client::new()
            .post(url)
            .header("x-api-key", api_key)
            .header("anthropic-version", &self.config.version);
        if let Some(beta_header) = beta_header {
            request_builder = request_builder.header("anthropic-beta", beta_header);
        }
        if let Some(session_id) = session_affinity_header {
            request_builder = request_builder.header("x-session-affinity", session_id);
        }
        let response = request_builder
            .json(&payload)
            .send()
            .map_err(|error| AiError::Http(error.to_string()))?;
        let status = response.status();
        if !status.is_success() {
            let body = response.text().unwrap_or_default();
            return Err(AiError::Http(format!("status={status}, body={body}")));
        }

        let body = response
            .text()
            .map_err(|error| AiError::InvalidResponse(error.to_string()))?;
        let raw_events = parse_anthropic_sse_text(&body).map_err(AiError::InvalidResponse)?;
        let result = process_anthropic_sse_events(
            &raw_events,
            anthropic_assistant_defaults(&model.provider, &model.api, &model.id),
        )
        .map_err(AiError::InvalidResponse)?;
        anthropic_stream_events_from_process_result(result).map_err(AiError::InvalidResponse)
    }
}

#[derive(Debug, Serialize)]
struct AnthropicRequest {
    model: String,
    max_tokens: u32,
    stream: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    system: Option<String>,
    messages: Vec<AnthropicMessage>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tools: Option<Vec<AnthropicTool>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    thinking: Option<AnthropicThinking>,
    #[serde(skip_serializing_if = "Option::is_none")]
    output_config: Option<AnthropicOutputConfig>,
}

impl AnthropicRequest {
    fn from_stream_request(request: StreamRequest, max_tokens: u32, is_oauth_token: bool) -> Self {
        let tool_cache_control = anthropic_tool_cache_control(&request);
        let mut system = Vec::new();
        let mut simple_messages = Vec::new();
        for message in request.messages {
            match message.role {
                MessageRole::System => system.push(message.content),
                MessageRole::User | MessageRole::Assistant => {
                    simple_messages.push(AnthropicMessage {
                        role: anthropic_role(&message.role).to_string(),
                        content: AnthropicMessageContent::Text(message.content),
                    })
                }
                MessageRole::Tool => simple_messages.push(AnthropicMessage {
                    role: "user".to_string(),
                    content: AnthropicMessageContent::Text(message.content),
                }),
            }
        }
        let messages = if request.rich_messages.is_empty() {
            simple_messages
        } else {
            convert_anthropic_rich_messages(&request.rich_messages, &request.model, is_oauth_token)
        };

        let reasoning = request.metadata.get("reasoning").and_then(parse_reasoning);
        let (thinking, output_config) = anthropic_thinking_payload(&request.model, reasoning);
        let tools = convert_anthropic_tools(
            &request.tools,
            is_oauth_token,
            anthropic_supports_eager_tool_input_streaming(&request.model),
            tool_cache_control,
        );

        Self {
            model: request.model.id,
            max_tokens,
            stream: true,
            system: if system.is_empty() {
                None
            } else {
                Some(system.join("\n\n"))
            },
            messages,
            tools,
            thinking,
            output_config,
        }
    }
}

#[derive(Debug, Serialize)]
struct AnthropicMessage {
    role: String,
    content: AnthropicMessageContent,
}

#[derive(Debug, Serialize)]
#[serde(untagged)]
enum AnthropicMessageContent {
    Text(String),
    Blocks(Vec<AnthropicContentBlock>),
}

#[derive(Debug, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum AnthropicContentBlock {
    Text {
        text: String,
    },
    Image {
        source: AnthropicImageSource,
    },
    Thinking {
        thinking: String,
        signature: String,
    },
    RedactedThinking {
        data: String,
    },
    ToolUse {
        id: String,
        name: String,
        input: Value,
    },
    ToolResult {
        tool_use_id: String,
        content: AnthropicToolResultContent,
        is_error: bool,
    },
}

#[derive(Debug, Serialize, Clone)]
struct AnthropicImageSource {
    r#type: String,
    media_type: String,
    data: String,
}

#[derive(Debug, Serialize)]
#[serde(untagged)]
enum AnthropicToolResultContent {
    Text(String),
    Blocks(Vec<AnthropicToolResultBlock>),
}

#[derive(Debug, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum AnthropicToolResultBlock {
    Text { text: String },
    Image { source: AnthropicImageSource },
}

#[derive(Debug, Serialize)]
struct AnthropicTool {
    name: String,
    description: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    eager_input_streaming: Option<bool>,
    input_schema: Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    cache_control: Option<AnthropicCacheControl>,
}

#[derive(Debug, Serialize, Clone)]
struct AnthropicCacheControl {
    r#type: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    ttl: Option<String>,
}

#[derive(Debug, Serialize)]
struct AnthropicThinking {
    r#type: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    budget_tokens: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    display: Option<String>,
}

#[derive(Debug, Serialize)]
struct AnthropicOutputConfig {
    effort: String,
}

#[derive(Debug, Clone)]
pub struct AnthropicRawSseEvent {
    pub event: String,
    pub data: String,
}

#[derive(Debug, Clone, PartialEq)]
pub enum AnthropicProcessedEvent {
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
    TextDelta {
        content_index: usize,
        delta: String,
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
        tool_call: RichToolCall,
    },
    Completed {
        stop_reason: AssistantStopReason,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub struct AnthropicSseProcessResult {
    pub assistant: RichAssistantMessage,
    pub events: Vec<AnthropicProcessedEvent>,
}

#[derive(Debug, Clone, Default)]
pub struct AnthropicSseProcessOptions {
    pub is_oauth_token: bool,
    pub tools: Vec<ToolDefinition>,
}

pub fn process_anthropic_sse_events(
    raw_events: &[AnthropicRawSseEvent],
    assistant: RichAssistantMessage,
) -> Result<AnthropicSseProcessResult, String> {
    process_anthropic_sse_events_with_options(
        raw_events,
        assistant,
        &AnthropicSseProcessOptions::default(),
    )
}

pub fn process_anthropic_sse_events_with_options(
    raw_events: &[AnthropicRawSseEvent],
    mut assistant: RichAssistantMessage,
    options: &AnthropicSseProcessOptions,
) -> Result<AnthropicSseProcessResult, String> {
    let mut events = Vec::new();
    let mut tool_states = Vec::<AnthropicToolStreamState>::new();
    let mut stopped = false;
    let mut saw_message_start = false;

    for raw_event in raw_events {
        if stopped {
            continue;
        }
        if raw_event.data.trim() == "[DONE]" {
            continue;
        }
        if raw_event.event == "error" {
            return Err(raw_event.data.clone());
        }
        let value = match parse_json_with_repair::<Value>(&raw_event.data) {
            Ok(value) => value,
            Err(error) => {
                if raw_event.event == "message_stop" {
                    stopped = true;
                    continue;
                }
                return Err(format!("Anthropic SSE JSON 无效：{error}"));
            }
        };

        match raw_event.event.as_str() {
            "message_start" => {
                saw_message_start = true;
                if let Some(id) = value
                    .get("message")
                    .and_then(|message| message.get("id"))
                    .and_then(Value::as_str)
                {
                    assistant.response_id = Some(id.to_string());
                }
                if let Some(usage) = value
                    .get("message")
                    .and_then(|message| message.get("usage"))
                {
                    assistant.usage = anthropic_usage_from_value(usage);
                }
            }
            "content_block_start" => {
                let index = value
                    .get("index")
                    .and_then(Value::as_u64)
                    .unwrap_or(assistant.content.len() as u64) as usize;
                let Some(block) = value.get("content_block") else {
                    continue;
                };
                match block.get("type").and_then(Value::as_str) {
                    Some("text") => ensure_anthropic_text_block(&mut assistant, index),
                    Some("thinking") => {
                        ensure_anthropic_content_slot(
                            &mut assistant,
                            index,
                            AssistantContentBlock::Thinking(ThinkingContent {
                                thinking: String::new(),
                                thinking_signature: Some(String::new()),
                                redacted: false,
                            }),
                        );
                        events.push(AnthropicProcessedEvent::ThinkingStart {
                            content_index: index,
                        });
                    }
                    Some("redacted_thinking") => {
                        ensure_anthropic_content_slot(
                            &mut assistant,
                            index,
                            AssistantContentBlock::Thinking(ThinkingContent {
                                thinking: "[Reasoning redacted]".to_string(),
                                thinking_signature: block
                                    .get("data")
                                    .and_then(Value::as_str)
                                    .map(str::to_string),
                                redacted: true,
                            }),
                        );
                        events.push(AnthropicProcessedEvent::ThinkingStart {
                            content_index: index,
                        });
                    }
                    Some("tool_use") => {
                        let initial_arguments =
                            anthropic_tool_arguments_from_input(block.get("input"));
                        let tool_call = RichToolCall {
                            id: block
                                .get("id")
                                .and_then(Value::as_str)
                                .unwrap_or_default()
                                .to_string(),
                            name: anthropic_inbound_tool_name(
                                block
                                    .get("name")
                                    .and_then(Value::as_str)
                                    .unwrap_or_default(),
                                options,
                            ),
                            arguments: initial_arguments.clone(),
                            thought_signature: None,
                        };
                        ensure_anthropic_content_slot(
                            &mut assistant,
                            index,
                            AssistantContentBlock::ToolCall(tool_call),
                        );
                        tool_states.push(AnthropicToolStreamState {
                            content_index: index,
                            initial_arguments,
                            partial_arguments: String::new(),
                        });
                        events.push(AnthropicProcessedEvent::ToolCallStart {
                            content_index: index,
                        });
                    }
                    _ => {}
                }
            }
            "content_block_delta" => {
                let Some(index) = value
                    .get("index")
                    .and_then(Value::as_u64)
                    .map(|v| v as usize)
                else {
                    continue;
                };
                let Some(delta) = value.get("delta") else {
                    continue;
                };
                match delta.get("type").and_then(Value::as_str) {
                    Some("text_delta") => {
                        let text = delta
                            .get("text")
                            .and_then(Value::as_str)
                            .unwrap_or_default();
                        ensure_anthropic_text_block(&mut assistant, index);
                        if let Some(AssistantContentBlock::Text(block)) =
                            assistant.content.get_mut(index)
                        {
                            block.text.push_str(text);
                            events.push(AnthropicProcessedEvent::TextDelta {
                                content_index: index,
                                delta: text.to_string(),
                            });
                        }
                    }
                    Some("input_json_delta") => {
                        let partial_json = delta
                            .get("partial_json")
                            .and_then(Value::as_str)
                            .unwrap_or_default();
                        let state = tool_states
                            .iter_mut()
                            .find(|state| state.content_index == index)
                            .ok_or_else(|| {
                                format!("Anthropic tool delta 缺少 content block：{index}")
                            })?;
                        state.partial_arguments.push_str(partial_json);
                        if let Some(AssistantContentBlock::ToolCall(tool_call)) =
                            assistant.content.get_mut(index)
                        {
                            tool_call.arguments =
                                anthropic_tool_arguments_from_partial(&state.partial_arguments);
                            events.push(AnthropicProcessedEvent::ToolCallDelta {
                                content_index: index,
                                delta: partial_json.to_string(),
                            });
                        }
                    }
                    Some("thinking_delta") => {
                        let thinking_delta = delta
                            .get("thinking")
                            .and_then(Value::as_str)
                            .unwrap_or_default();
                        if let Some(AssistantContentBlock::Thinking(thinking)) =
                            assistant.content.get_mut(index)
                        {
                            thinking.thinking.push_str(thinking_delta);
                            events.push(AnthropicProcessedEvent::ThinkingDelta {
                                content_index: index,
                                delta: thinking_delta.to_string(),
                            });
                        }
                    }
                    Some("signature_delta") => {
                        let signature_delta = delta
                            .get("signature")
                            .and_then(Value::as_str)
                            .unwrap_or_default();
                        if let Some(AssistantContentBlock::Thinking(thinking)) =
                            assistant.content.get_mut(index)
                        {
                            thinking
                                .thinking_signature
                                .get_or_insert_with(String::new)
                                .push_str(signature_delta);
                        }
                    }
                    _ => {}
                }
            }
            "content_block_stop" => {
                let Some(index) = value
                    .get("index")
                    .and_then(Value::as_u64)
                    .map(|v| v as usize)
                else {
                    continue;
                };
                match assistant.content.get_mut(index) {
                    Some(AssistantContentBlock::ToolCall(tool_call)) => {
                        let Some(state) = tool_states
                            .iter()
                            .find(|state| state.content_index == index)
                        else {
                            continue;
                        };
                        tool_call.arguments = if state.partial_arguments.trim().is_empty() {
                            state.initial_arguments.clone()
                        } else {
                            anthropic_tool_arguments_from_partial(&state.partial_arguments)
                        };
                        events.push(AnthropicProcessedEvent::ToolCallEnd {
                            content_index: index,
                            tool_call: tool_call.clone(),
                        });
                    }
                    Some(AssistantContentBlock::Thinking(thinking)) => {
                        events.push(AnthropicProcessedEvent::ThinkingEnd {
                            content_index: index,
                            content: thinking.thinking.clone(),
                        });
                    }
                    _ => {}
                }
            }
            "message_delta" => {
                if let Some(stop_reason) = value
                    .get("delta")
                    .and_then(|delta| delta.get("stop_reason"))
                    .and_then(Value::as_str)
                {
                    assistant.stop_reason = map_anthropic_stop_reason(stop_reason)?;
                }
                if let Some(usage) = value.get("usage") {
                    update_anthropic_usage_from_value(&mut assistant.usage, usage);
                }
            }
            "message_stop" => {
                stopped = true;
                events.push(AnthropicProcessedEvent::Completed {
                    stop_reason: assistant.stop_reason.clone(),
                });
            }
            _ => {}
        }
    }

    if saw_message_start && !stopped {
        return Err("Anthropic stream ended before message_stop".to_string());
    }

    Ok(AnthropicSseProcessResult { assistant, events })
}

pub fn parse_anthropic_sse_text(input: &str) -> Result<Vec<AnthropicRawSseEvent>, String> {
    let mut events = Vec::new();
    let mut event_name: Option<String> = None;
    let mut data_lines: Vec<String> = Vec::new();

    for line in input.lines() {
        let line = line.strip_suffix('\r').unwrap_or(line);
        let line = line.trim_start();
        if line.is_empty() {
            flush_anthropic_sse_event(&mut events, &mut event_name, &mut data_lines);
            continue;
        }
        if line.starts_with(':') {
            continue;
        }
        if let Some(value) = line.strip_prefix("event:") {
            event_name = Some(value.trim_start().to_string());
            continue;
        }
        if let Some(value) = line.strip_prefix("data:") {
            data_lines.push(value.trim_start().to_string());
        }
    }

    flush_anthropic_sse_event(&mut events, &mut event_name, &mut data_lines);
    Ok(events)
}

fn flush_anthropic_sse_event(
    events: &mut Vec<AnthropicRawSseEvent>,
    event_name: &mut Option<String>,
    data_lines: &mut Vec<String>,
) {
    if data_lines.is_empty() {
        *event_name = None;
        return;
    }
    events.push(AnthropicRawSseEvent {
        event: event_name.take().unwrap_or_else(|| "message".to_string()),
        data: data_lines.join("\n"),
    });
    data_lines.clear();
}

fn anthropic_assistant_defaults(provider: &str, api: &str, model: &str) -> RichAssistantMessage {
    RichAssistantMessage {
        content: Vec::new(),
        api: api.to_string(),
        provider: provider.to_string(),
        model: model.to_string(),
        response_model: None,
        response_id: None,
        usage: Default::default(),
        stop_reason: AssistantStopReason::Stop,
        error_message: None,
        diagnostics: Vec::new(),
        timestamp_millis: 0,
    }
}

fn anthropic_assistant_text(assistant: &RichAssistantMessage) -> String {
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

fn anthropic_stream_events_from_process_result(
    result: AnthropicSseProcessResult,
) -> Result<Vec<StreamEvent>, String> {
    let mut events = Vec::new();
    for event in &result.events {
        match event {
            AnthropicProcessedEvent::ThinkingStart { content_index } => {
                events.push(StreamEvent::ThinkingStart {
                    content_index: *content_index,
                });
            }
            AnthropicProcessedEvent::ThinkingDelta {
                content_index,
                delta,
            } => {
                events.push(StreamEvent::ThinkingDelta {
                    content_index: *content_index,
                    delta: delta.clone(),
                });
            }
            AnthropicProcessedEvent::ThinkingEnd {
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
            AnthropicProcessedEvent::TextDelta { delta, .. } => {
                events.push(StreamEvent::TextDelta {
                    text: delta.clone(),
                });
            }
            AnthropicProcessedEvent::ToolCallStart { content_index } => {
                events.push(StreamEvent::ToolCallStart {
                    content_index: *content_index,
                });
            }
            AnthropicProcessedEvent::ToolCallDelta {
                content_index,
                delta,
            } => {
                events.push(StreamEvent::ToolCallDelta {
                    content_index: *content_index,
                    delta: delta.clone(),
                });
            }
            AnthropicProcessedEvent::ToolCallEnd {
                content_index,
                tool_call,
            } => {
                events.push(StreamEvent::ToolCallEnd {
                    content_index: *content_index,
                    tool_call: stream_tool_call_from_rich(tool_call),
                });
            }
            _ => {}
        }
    }
    if result.assistant.usage.total_tokens > 0 {
        events.push(StreamEvent::Usage {
            usage: result.assistant.usage.clone(),
        });
    }

    let content = anthropic_assistant_text(&result.assistant);
    let has_tool_calls = result
        .assistant
        .content
        .iter()
        .any(|block| matches!(block, AssistantContentBlock::ToolCall(_)));
    if content.is_empty() && !has_tool_calls {
        return Err("content text 缺失".to_string());
    }
    events.push(StreamEvent::Finished {
        message: Message {
            role: MessageRole::Assistant,
            content,
        },
    });
    Ok(events)
}

fn stream_tool_call_from_rich(tool_call: &RichToolCall) -> StreamToolCall {
    StreamToolCall {
        id: tool_call.id.clone(),
        name: tool_call.name.clone(),
        arguments: tool_call.arguments.clone(),
        thought_signature: tool_call.thought_signature.clone(),
    }
}

#[derive(Debug, Clone)]
struct AnthropicToolStreamState {
    content_index: usize,
    initial_arguments: std::collections::BTreeMap<String, Value>,
    partial_arguments: String,
}

fn ensure_anthropic_text_block(assistant: &mut RichAssistantMessage, index: usize) {
    if matches!(
        assistant.content.get(index),
        Some(AssistantContentBlock::Text(_))
    ) {
        return;
    }
    ensure_anthropic_content_slot(
        assistant,
        index,
        AssistantContentBlock::Text(TextContent {
            text: String::new(),
            text_signature: None,
        }),
    );
}

fn ensure_anthropic_content_slot(
    assistant: &mut RichAssistantMessage,
    index: usize,
    block: AssistantContentBlock,
) {
    while assistant.content.len() <= index {
        assistant
            .content
            .push(AssistantContentBlock::Text(TextContent {
                text: String::new(),
                text_signature: None,
            }));
    }
    assistant.content[index] = block;
}

fn anthropic_tool_arguments_from_partial(
    partial_arguments: &str,
) -> std::collections::BTreeMap<String, Value> {
    match parse_streaming_json(Some(partial_arguments)) {
        Value::Object(arguments) => arguments.into_iter().collect(),
        _ => Default::default(),
    }
}

fn anthropic_tool_arguments_from_input(
    input: Option<&Value>,
) -> std::collections::BTreeMap<String, Value> {
    match input {
        Some(Value::Object(arguments)) => arguments.clone().into_iter().collect(),
        _ => Default::default(),
    }
}

fn map_anthropic_stop_reason(reason: &str) -> Result<AssistantStopReason, String> {
    match reason {
        "end_turn" | "pause_turn" | "stop_sequence" => Ok(AssistantStopReason::Stop),
        "tool_use" => Ok(AssistantStopReason::ToolUse),
        "max_tokens" => Ok(AssistantStopReason::Length),
        "refusal" | "sensitive" => Ok(AssistantStopReason::Error),
        _ => Err(format!("Unhandled stop reason: {reason}")),
    }
}

fn anthropic_usage_from_value(value: &Value) -> crate::types::Usage {
    let input = value
        .get("input_tokens")
        .and_then(Value::as_u64)
        .unwrap_or_default();
    let output = value
        .get("output_tokens")
        .and_then(Value::as_u64)
        .unwrap_or_default();
    let cache_read = value
        .get("cache_read_input_tokens")
        .and_then(Value::as_u64)
        .unwrap_or_default();
    let cache_write = value
        .get("cache_creation_input_tokens")
        .and_then(Value::as_u64)
        .unwrap_or_default();
    crate::types::Usage {
        input,
        output,
        cache_read,
        cache_write,
        total_tokens: input + output + cache_read + cache_write,
        cost: Default::default(),
    }
}

fn update_anthropic_usage_from_value(usage: &mut crate::types::Usage, value: &Value) {
    if let Some(input) = value.get("input_tokens").and_then(Value::as_u64) {
        usage.input = input;
    }
    if let Some(output) = value.get("output_tokens").and_then(Value::as_u64) {
        usage.output = output;
    }
    if let Some(cache_read) = value.get("cache_read_input_tokens").and_then(Value::as_u64) {
        usage.cache_read = cache_read;
    }
    if let Some(cache_write) = value
        .get("cache_creation_input_tokens")
        .and_then(Value::as_u64)
    {
        usage.cache_write = cache_write;
    }
    usage.total_tokens = usage.input + usage.output + usage.cache_read + usage.cache_write;
}

fn anthropic_role(role: &MessageRole) -> &'static str {
    match role {
        MessageRole::Assistant => "assistant",
        _ => "user",
    }
}

fn convert_anthropic_rich_messages(
    messages: &[RichMessage],
    model: &Model,
    is_oauth_token: bool,
) -> Vec<AnthropicMessage> {
    let transformed_messages = transform_messages(
        messages,
        model,
        Some(normalize_anthropic_tool_call_id_for_model),
    );
    let mut params = Vec::new();
    let mut index = 0usize;

    while index < transformed_messages.len() {
        match transformed_messages[index].clone() {
            RichMessage::User(user) => {
                if let Some(content) = anthropic_user_content(user.content) {
                    params.push(AnthropicMessage {
                        role: "user".to_string(),
                        content,
                    });
                }
            }
            RichMessage::Assistant(assistant) => {
                let mut blocks = Vec::new();
                for block in assistant.content {
                    match block {
                        AssistantContentBlock::Text(text) => {
                            if text.text.trim().is_empty() {
                                continue;
                            }
                            blocks.push(AnthropicContentBlock::Text {
                                text: sanitize_surrogates(&text.text),
                            });
                        }
                        AssistantContentBlock::Thinking(thinking) => {
                            if thinking.redacted {
                                if let Some(signature) = thinking.thinking_signature {
                                    blocks.push(AnthropicContentBlock::RedactedThinking {
                                        data: signature,
                                    });
                                }
                                continue;
                            }
                            if thinking.thinking.trim().is_empty() {
                                continue;
                            }
                            match thinking.thinking_signature {
                                Some(signature) if !signature.trim().is_empty() => {
                                    blocks.push(AnthropicContentBlock::Thinking {
                                        thinking: sanitize_surrogates(&thinking.thinking),
                                        signature,
                                    });
                                }
                                _ => blocks.push(AnthropicContentBlock::Text {
                                    text: sanitize_surrogates(&thinking.thinking),
                                }),
                            }
                        }
                        AssistantContentBlock::ToolCall(tool_call) => {
                            let input = serde_json::to_value(tool_call.arguments)
                                .unwrap_or_else(|_| json!({}));
                            blocks.push(AnthropicContentBlock::ToolUse {
                                id: tool_call.id,
                                name: if is_oauth_token {
                                    to_claude_code_tool_name(&tool_call.name)
                                } else {
                                    tool_call.name
                                },
                                input,
                            });
                        }
                    }
                }
                if !blocks.is_empty() {
                    params.push(AnthropicMessage {
                        role: "assistant".to_string(),
                        content: AnthropicMessageContent::Blocks(blocks),
                    });
                }
            }
            RichMessage::ToolResult(tool_result) => {
                let mut blocks = vec![anthropic_tool_result_block(tool_result)];
                let mut next_index = index + 1;
                while next_index < transformed_messages.len() {
                    match transformed_messages[next_index].clone() {
                        RichMessage::ToolResult(next_tool_result) => {
                            blocks.push(anthropic_tool_result_block(next_tool_result));
                            next_index += 1;
                        }
                        _ => break,
                    }
                }
                params.push(AnthropicMessage {
                    role: "user".to_string(),
                    content: AnthropicMessageContent::Blocks(blocks),
                });
                index = next_index - 1;
            }
        }
        index += 1;
    }

    params
}

fn normalize_anthropic_tool_call_id_for_model(
    id: &str,
    _model: &Model,
    _source: &RichAssistantMessage,
) -> String {
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

fn anthropic_user_content(content: UserMessageContent) -> Option<AnthropicMessageContent> {
    match content {
        UserMessageContent::Text(text) => (!text.trim().is_empty())
            .then(|| AnthropicMessageContent::Text(sanitize_surrogates(&text))),
        UserMessageContent::Blocks(blocks) => {
            let blocks = blocks
                .into_iter()
                .filter_map(anthropic_user_content_block)
                .collect::<Vec<_>>();
            (!blocks.is_empty()).then_some(AnthropicMessageContent::Blocks(blocks))
        }
    }
}

fn anthropic_user_content_block(block: UserContentBlock) -> Option<AnthropicContentBlock> {
    match block {
        UserContentBlock::Text(text) => {
            (!text.text.trim().is_empty()).then(|| AnthropicContentBlock::Text {
                text: sanitize_surrogates(&text.text),
            })
        }
        UserContentBlock::Image(image) => Some(AnthropicContentBlock::Image {
            source: AnthropicImageSource {
                r#type: "base64".to_string(),
                media_type: image.mime_type,
                data: image.data,
            },
        }),
    }
}

fn anthropic_tool_result_block(
    tool_result: crate::conversation::ToolResultMessage,
) -> AnthropicContentBlock {
    AnthropicContentBlock::ToolResult {
        tool_use_id: tool_result.tool_call_id,
        content: anthropic_tool_result_content(tool_result.content),
        is_error: tool_result.is_error,
    }
}

fn anthropic_tool_result_content(content: Vec<UserContentBlock>) -> AnthropicToolResultContent {
    let has_images = content
        .iter()
        .any(|block| matches!(block, UserContentBlock::Image(_)));
    if !has_images {
        let text = content
            .into_iter()
            .filter_map(|block| match block {
                UserContentBlock::Text(text) => Some(sanitize_surrogates(&text.text)),
                UserContentBlock::Image(_) => None,
            })
            .collect::<Vec<_>>()
            .join("\n");
        return AnthropicToolResultContent::Text(text);
    }

    let mut has_text = false;
    let mut blocks = content
        .into_iter()
        .filter_map(|block| match block {
            UserContentBlock::Text(text) => {
                has_text = has_text || !text.text.trim().is_empty();
                (!text.text.trim().is_empty()).then(|| AnthropicToolResultBlock::Text {
                    text: sanitize_surrogates(&text.text),
                })
            }
            UserContentBlock::Image(image) => Some(AnthropicToolResultBlock::Image {
                source: AnthropicImageSource {
                    r#type: "base64".to_string(),
                    media_type: image.mime_type,
                    data: image.data,
                },
            }),
        })
        .collect::<Vec<_>>();
    if !has_text {
        blocks.insert(
            0,
            AnthropicToolResultBlock::Text {
                text: "(see attached image)".to_string(),
            },
        );
    }
    AnthropicToolResultContent::Blocks(blocks)
}

fn convert_anthropic_tools(
    tools: &[ToolDefinition],
    is_oauth_token: bool,
    supports_eager_tool_input_streaming: bool,
    cache_control: Option<AnthropicCacheControl>,
) -> Option<Vec<AnthropicTool>> {
    if tools.is_empty() {
        return None;
    }
    let last_tool_index = tools.len() - 1;
    Some(
        tools
            .iter()
            .enumerate()
            .map(|(index, tool)| {
                let properties = tool
                    .parameters
                    .get("properties")
                    .cloned()
                    .unwrap_or_else(|| json!({}));
                let required = tool
                    .parameters
                    .get("required")
                    .cloned()
                    .unwrap_or_else(|| json!([]));
                AnthropicTool {
                    name: if is_oauth_token {
                        to_claude_code_tool_name(&tool.name)
                    } else {
                        tool.name.clone()
                    },
                    description: tool.description.clone(),
                    eager_input_streaming: supports_eager_tool_input_streaming.then_some(true),
                    input_schema: json!({
                        "type": "object",
                        "properties": properties,
                        "required": required,
                    }),
                    cache_control: (index == last_tool_index)
                        .then(|| cache_control.clone())
                        .flatten(),
                }
            })
            .collect(),
    )
}

fn anthropic_is_oauth_token(api_key: &str) -> bool {
    api_key.contains("sk-ant-oat")
}

fn to_claude_code_tool_name(name: &str) -> String {
    [
        "Read",
        "Write",
        "Edit",
        "Bash",
        "Grep",
        "Glob",
        "AskUserQuestion",
        "EnterPlanMode",
        "ExitPlanMode",
        "KillShell",
        "NotebookEdit",
        "Skill",
        "Task",
        "TaskOutput",
        "TodoWrite",
        "WebFetch",
        "WebSearch",
    ]
    .into_iter()
    .find(|canonical| canonical.eq_ignore_ascii_case(name))
    .map(str::to_string)
    .unwrap_or_else(|| name.to_string())
}

fn anthropic_inbound_tool_name(name: &str, options: &AnthropicSseProcessOptions) -> String {
    if !options.is_oauth_token {
        return name.to_string();
    }
    from_claude_code_tool_name(name, &options.tools)
}

fn from_claude_code_tool_name(name: &str, tools: &[ToolDefinition]) -> String {
    tools
        .iter()
        .find(|tool| tool.name.eq_ignore_ascii_case(name))
        .map(|tool| tool.name.clone())
        .unwrap_or_else(|| name.to_string())
}

fn anthropic_supports_eager_tool_input_streaming(model: &crate::types::Model) -> bool {
    model
        .compat
        .get("supportsEagerToolInputStreaming")
        .and_then(Value::as_bool)
        .unwrap_or(true)
}

fn anthropic_tool_cache_control(request: &StreamRequest) -> Option<AnthropicCacheControl> {
    let supports_cache_control_on_tools = request
        .model
        .compat
        .get("supportsCacheControlOnTools")
        .and_then(Value::as_bool)
        .unwrap_or(true);
    if !supports_cache_control_on_tools {
        return None;
    }

    let cache_retention = request
        .metadata
        .get("cacheRetention")
        .and_then(Value::as_str)
        .unwrap_or("short");
    if cache_retention == "none" {
        return None;
    }

    let ttl = if cache_retention == "long"
        && request
            .model
            .compat
            .get("supportsLongCacheRetention")
            .and_then(Value::as_bool)
            .unwrap_or(true)
    {
        Some("1h".to_string())
    } else {
        None
    };

    Some(AnthropicCacheControl {
        r#type: "ephemeral".to_string(),
        ttl,
    })
}

fn anthropic_beta_header_for_request(request: &StreamRequest) -> Option<&'static str> {
    (!request.tools.is_empty() && !anthropic_supports_eager_tool_input_streaming(&request.model))
        .then_some(FINE_GRAINED_TOOL_STREAMING_BETA)
}

fn anthropic_session_affinity_header(request: &StreamRequest) -> Option<String> {
    let enabled = request
        .model
        .compat
        .get("sendSessionAffinityHeaders")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    if !enabled {
        return None;
    }

    request
        .metadata
        .get("sessionId")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|session_id| !session_id.is_empty())
        .map(str::to_string)
}

fn parse_reasoning(value: &Value) -> Option<ModelThinkingLevel> {
    match value.as_str()? {
        "off" => Some(ModelThinkingLevel::Off),
        "minimal" => Some(ModelThinkingLevel::Minimal),
        "low" => Some(ModelThinkingLevel::Low),
        "medium" => Some(ModelThinkingLevel::Medium),
        "high" => Some(ModelThinkingLevel::High),
        "xhigh" => Some(ModelThinkingLevel::XHigh),
        _ => None,
    }
}

fn anthropic_thinking_payload(
    model: &crate::types::Model,
    reasoning: Option<ModelThinkingLevel>,
) -> (Option<AnthropicThinking>, Option<AnthropicOutputConfig>) {
    if !model
        .reasoning
        .as_ref()
        .is_some_and(|reasoning| reasoning.enabled)
    {
        return (None, None);
    }

    let Some(reasoning) = reasoning else {
        return (
            Some(AnthropicThinking {
                r#type: "disabled".to_string(),
                budget_tokens: None,
                display: None,
            }),
            None,
        );
    };
    if reasoning == ModelThinkingLevel::Off {
        return (
            Some(AnthropicThinking {
                r#type: "disabled".to_string(),
                budget_tokens: None,
                display: None,
            }),
            None,
        );
    }

    let display = Some("summarized".to_string());
    if anthropic_uses_adaptive_thinking(model) {
        return (
            Some(AnthropicThinking {
                r#type: "adaptive".to_string(),
                budget_tokens: None,
                display,
            }),
            Some(AnthropicOutputConfig {
                effort: anthropic_effort_for_level(model, reasoning).to_string(),
            }),
        );
    }

    (
        Some(AnthropicThinking {
            r#type: "enabled".to_string(),
            budget_tokens: Some(1024),
            display,
        }),
        None,
    )
}

fn anthropic_uses_adaptive_thinking(model: &crate::types::Model) -> bool {
    if let Some(force_adaptive) = model
        .compat
        .get("forceAdaptiveThinking")
        .and_then(Value::as_bool)
    {
        return force_adaptive;
    }

    model.id.contains("opus-4-6") || model.id.contains("opus-4-7") || model.id.contains("mythos")
}

fn anthropic_effort_for_level(model: &crate::types::Model, level: ModelThinkingLevel) -> &str {
    if let Some(Some(mapped)) = model.thinking_level_map.get(&level) {
        return mapped.as_str();
    }
    match level {
        ModelThinkingLevel::Minimal | ModelThinkingLevel::Low => "low",
        ModelThinkingLevel::Medium => "medium",
        ModelThinkingLevel::High => "high",
        ModelThinkingLevel::XHigh => "high",
        ModelThinkingLevel::Off => "high",
    }
}

fn anthropic_provider_base_url(provider: &str, raw_base_url: &str) -> AiResult<String> {
    if is_cloudflare_provider(provider) {
        resolve_cloudflare_base_url_from_str(provider, raw_base_url)
    } else {
        Ok(raw_base_url.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::conversation::{
        AssistantContentBlock, ImageContent, RichMessage, TextContent, ThinkingContent,
        UserContentBlock, UserMessage, UserMessageContent,
    };
    use crate::types::{Model, ModelInputKind, ModelReasoning, ModelThinkingLevel, Usage};
    use std::collections::BTreeMap;
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::sync::mpsc;
    use std::thread;

    #[test]
    fn keeps_normal_anthropic_base_url() {
        assert_eq!(
            anthropic_provider_base_url("anthropic", "https://api.anthropic.com")
                .expect("base url"),
            "https://api.anthropic.com"
        );
    }

    #[test]
    fn accepts_resolved_cloudflare_anthropic_base_url() {
        assert_eq!(
            anthropic_provider_base_url(
                "cloudflare-ai-gateway",
                "https://gateway.ai.cloudflare.com/v1/account/gateway/anthropic"
            )
            .expect("base url"),
            "https://gateway.ai.cloudflare.com/v1/account/gateway/anthropic"
        );
    }

    #[test]
    fn builds_streaming_anthropic_request_like_pi() {
        let payload = AnthropicRequest::from_stream_request(
            StreamRequest {
                model: reasoning_model("claude-haiku-4-5"),
                messages: vec![Message {
                    role: MessageRole::User,
                    content: "Hello".to_string(),
                }],
                rich_messages: Vec::new(),
                tools: Vec::new(),
                metadata: Default::default(),
            },
            4096,
            false,
        );

        let value = serde_json::to_value(payload).expect("payload json");
        assert_eq!(value.get("stream"), Some(&json!(true)));
    }

    #[test]
    fn fireworks_anthropic_request_sends_session_affinity_header_like_pi() {
        let listener = TcpListener::bind("127.0.0.1:0").expect("listener");
        let address = listener.local_addr().expect("address");
        let (request_sender, request_receiver) = mpsc::channel();
        let server = thread::spawn(move || {
            let (mut stream, _) = listener.accept().expect("accept");
            let mut request = Vec::new();
            let mut buffer = [0_u8; 1024];
            loop {
                let read = stream.read(&mut buffer).expect("read");
                if read == 0 {
                    break;
                }
                request.extend_from_slice(&buffer[..read]);
                if request.windows(4).any(|window| window == b"\r\n\r\n") {
                    break;
                }
            }
            request_sender
                .send(String::from_utf8_lossy(&request).to_string())
                .expect("send request");

            let body = concat!(
                "event: message_start\n",
                "data: {\"type\":\"message_start\",\"message\":{\"id\":\"msg_1\",\"usage\":{\"input_tokens\":1}}}\n\n",
                "event: content_block_start\n",
                "data: {\"type\":\"content_block_start\",\"index\":0,\"content_block\":{\"type\":\"text\",\"text\":\"\"}}\n\n",
                "event: content_block_delta\n",
                "data: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"text_delta\",\"text\":\"hi\"}}\n\n",
                "event: message_delta\n",
                "data: {\"type\":\"message_delta\",\"delta\":{\"stop_reason\":\"end_turn\"},\"usage\":{\"output_tokens\":1}}\n\n",
                "event: message_stop\n",
                "data: {\"type\":\"message_stop\"}\n\n",
            );
            let response = format!(
                "HTTP/1.1 200 OK\r\ncontent-type: text/event-stream\r\ncontent-length: {}\r\n\r\n{}",
                body.len(),
                body
            );
            stream.write_all(response.as_bytes()).expect("write");
        });

        let mut model = Model {
            id: "accounts/fireworks/models/kimi-k2p6".to_string(),
            provider: "fireworks".to_string(),
            api: "anthropic-messages".to_string(),
            display_name: "Kimi K2.6".to_string(),
            base_url: Some(format!("http://{address}")),
            ..Model::default()
        };
        model.compat.insert(
            "sendSessionAffinityHeaders".to_string(),
            serde_json::json!(true),
        );
        let provider = AnthropicMessagesProvider::new(AnthropicMessagesConfig {
            api_key: Some("fireworks-key".to_string()),
            base_url: "https://api.anthropic.com".to_string(),
            version: "2023-06-01".to_string(),
            max_tokens: 4096,
        });

        provider
            .stream(StreamRequest {
                model,
                messages: vec![Message {
                    role: MessageRole::User,
                    content: "Hello".to_string(),
                }],
                rich_messages: Vec::new(),
                tools: Vec::new(),
                metadata: BTreeMap::from([("sessionId".to_string(), json!("session-1"))]),
            })
            .expect("stream");
        server.join().expect("server");
        let request_text = request_receiver
            .recv()
            .expect("request text")
            .to_ascii_lowercase();
        assert!(request_text.starts_with("post /v1/messages http/1.1"));
        assert!(request_text.contains("x-session-affinity: session-1"));
    }

    #[test]
    fn session_affinity_header_requires_enabled_compat_and_session_id_like_pi() {
        let mut model = Model {
            id: "accounts/fireworks/models/kimi-k2p6".to_string(),
            provider: "fireworks".to_string(),
            api: "anthropic-messages".to_string(),
            display_name: "Kimi K2.6".to_string(),
            ..Model::default()
        };
        model.compat.insert(
            "sendSessionAffinityHeaders".to_string(),
            serde_json::json!(true),
        );

        assert_eq!(
            anthropic_session_affinity_header(&StreamRequest {
                model: model.clone(),
                messages: Vec::new(),
                rich_messages: Vec::new(),
                tools: Vec::new(),
                metadata: BTreeMap::from([("sessionId".to_string(), json!("session-1"))]),
            }),
            Some("session-1".to_string())
        );

        assert_eq!(
            anthropic_session_affinity_header(&StreamRequest {
                model: model.clone(),
                messages: Vec::new(),
                rich_messages: Vec::new(),
                tools: Vec::new(),
                metadata: BTreeMap::from([("sessionId".to_string(), json!("  "))]),
            }),
            None
        );

        model.compat.insert(
            "sendSessionAffinityHeaders".to_string(),
            serde_json::json!(false),
        );
        assert_eq!(
            anthropic_session_affinity_header(&StreamRequest {
                model,
                messages: Vec::new(),
                rich_messages: Vec::new(),
                tools: Vec::new(),
                metadata: BTreeMap::from([("sessionId".to_string(), json!("session-1"))]),
            }),
            None
        );
    }

    #[test]
    fn builds_anthropic_request_prefers_rich_messages_like_pi() {
        let payload = AnthropicRequest::from_stream_request(
            StreamRequest {
                model: Model {
                    id: "claude-sonnet-4-5".to_string(),
                    provider: "anthropic".to_string(),
                    api: "anthropic-messages".to_string(),
                    display_name: "Claude Sonnet 4.5".to_string(),
                    input: vec![ModelInputKind::Image],
                    ..Model::default()
                },
                messages: vec![
                    Message {
                        role: MessageRole::System,
                        content: "system".to_string(),
                    },
                    Message {
                        role: MessageRole::User,
                        content: "fallback simple".to_string(),
                    },
                ],
                rich_messages: vec![
                    RichMessage::User(UserMessage {
                        content: UserMessageContent::Blocks(vec![
                            UserContentBlock::Text(TextContent {
                                text: "rich hello".to_string(),
                                text_signature: None,
                            }),
                            UserContentBlock::Image(ImageContent {
                                data: "abc".to_string(),
                                mime_type: "image/png".to_string(),
                            }),
                        ]),
                        timestamp_millis: 1,
                    }),
                    RichMessage::Assistant(RichAssistantMessage {
                        content: vec![AssistantContentBlock::Thinking(ThinkingContent {
                            thinking: "reasoning".to_string(),
                            thinking_signature: Some("sig".to_string()),
                            redacted: false,
                        })],
                        provider: "anthropic".to_string(),
                        api: "anthropic-messages".to_string(),
                        model: "claude-sonnet-4-5".to_string(),
                        response_model: None,
                        response_id: None,
                        usage: Usage::default(),
                        stop_reason: AssistantStopReason::Stop,
                        error_message: None,
                        diagnostics: Vec::new(),
                        timestamp_millis: 1,
                    }),
                ],
                tools: Vec::new(),
                metadata: Default::default(),
            },
            4096,
            false,
        );

        let value = serde_json::to_value(payload).expect("payload json");
        assert!(!value.to_string().contains("fallback simple"));
        assert_eq!(value["system"], "system");
        assert_eq!(value["messages"][0]["role"], "user");
        assert_eq!(value["messages"][0]["content"][0]["type"], "text");
        assert_eq!(value["messages"][0]["content"][0]["text"], "rich hello");
        assert_eq!(value["messages"][0]["content"][1]["type"], "image");
        assert_eq!(
            value["messages"][0]["content"][1]["source"]["media_type"],
            "image/png"
        );
        assert_eq!(value["messages"][0]["content"][1]["source"]["data"], "abc");
        assert_eq!(value["messages"][1]["role"], "assistant");
        assert_eq!(value["messages"][1]["content"][0]["type"], "thinking");
        assert_eq!(value["messages"][1]["content"][0]["thinking"], "reasoning");
        assert_eq!(value["messages"][1]["content"][0]["signature"], "sig");
    }

    #[test]
    fn parses_anthropic_sse_text_into_raw_events_like_pi() {
        let raw = parse_anthropic_sse_text(
            "event: message_start\n\
             data: {\"type\":\"message_start\",\"message\":{\"id\":\"msg_1\",\"usage\":{\"input_tokens\":1}}}\n\n\
             event: message_delta\n\
             data: {\"type\":\"message_delta\",\"delta\":{\"stop_reason\":\"end_turn\"},\"usage\":{\"output_tokens\":2}}\n\n\
             event: message_stop\n\
             data: {\"type\":\"message_stop\"}\n\n",
        )
        .expect("sse parse");

        assert_eq!(raw.len(), 3);
        assert_eq!(raw[0].event, "message_start");
        assert!(raw[0].data.contains("\"msg_1\""));
        assert_eq!(raw[2].event, "message_stop");
    }

    #[test]
    fn converts_anthropic_processed_events_to_stream_events_like_pi() {
        let raw = parse_anthropic_sse_text(
            "event: message_start\n\
             data: {\"type\":\"message_start\",\"message\":{\"id\":\"msg_1\",\"usage\":{\"input_tokens\":3}}}\n\n\
             event: content_block_start\n\
             data: {\"type\":\"content_block_start\",\"index\":0,\"content_block\":{\"type\":\"text\",\"text\":\"\"}}\n\n\
             event: content_block_delta\n\
             data: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"text_delta\",\"text\":\"hel\"}}\n\n\
             event: content_block_delta\n\
             data: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"text_delta\",\"text\":\"lo\"}}\n\n\
             event: message_delta\n\
             data: {\"type\":\"message_delta\",\"delta\":{\"stop_reason\":\"end_turn\"},\"usage\":{\"output_tokens\":2}}\n\n\
             event: message_stop\n\
             data: {\"type\":\"message_stop\"}\n\n",
        )
        .expect("sse parse");
        let result = process_anthropic_sse_events(
            &raw,
            anthropic_assistant_defaults("anthropic", "anthropic-messages", "claude-haiku-4-5"),
        )
        .expect("process");

        let events = anthropic_stream_events_from_process_result(result).expect("stream events");

        assert!(matches!(
            &events[0],
            StreamEvent::TextDelta { text } if text == "hel"
        ));
        assert!(matches!(
            &events[1],
            StreamEvent::TextDelta { text } if text == "lo"
        ));
        assert!(matches!(
            &events[2],
            StreamEvent::Usage { usage } if usage.input == 3 && usage.output == 2
        ));
        assert!(matches!(
            events.last().expect("finished"),
            StreamEvent::Finished { message } if message.content == "hello"
        ));
    }

    #[test]
    fn converts_anthropic_tool_calls_to_stream_events_like_pi() {
        let raw = parse_anthropic_sse_text(
            "event: message_start\n\
             data: {\"type\":\"message_start\",\"message\":{\"id\":\"msg_1\",\"usage\":{\"input_tokens\":3}}}\n\n\
             event: content_block_start\n\
             data: {\"type\":\"content_block_start\",\"index\":0,\"content_block\":{\"type\":\"tool_use\",\"id\":\"toolu_1\",\"name\":\"edit\",\"input\":{}}}\n\n\
             event: content_block_delta\n\
             data: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"input_json_delta\",\"partial_json\":\"{\\\"path\\\":\"}}\n\n\
             event: content_block_delta\n\
             data: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"input_json_delta\",\"partial_json\":\"\\\"README.md\\\"}\"}}\n\n\
             event: content_block_stop\n\
             data: {\"type\":\"content_block_stop\",\"index\":0}\n\n\
             event: message_delta\n\
             data: {\"type\":\"message_delta\",\"delta\":{\"stop_reason\":\"tool_use\"},\"usage\":{\"output_tokens\":2}}\n\n\
             event: message_stop\n\
             data: {\"type\":\"message_stop\"}\n\n",
        )
        .expect("sse parse");
        let result = process_anthropic_sse_events(
            &raw,
            anthropic_assistant_defaults("anthropic", "anthropic-messages", "claude-haiku-4-5"),
        )
        .expect("process");

        let events = anthropic_stream_events_from_process_result(result).expect("stream events");

        assert!(matches!(
            &events[0],
            StreamEvent::ToolCallStart { content_index } if *content_index == 0
        ));
        assert!(matches!(
            &events[1],
            StreamEvent::ToolCallDelta { content_index, delta }
                if *content_index == 0 && delta == "{\"path\":"
        ));
        assert!(matches!(
            &events[2],
            StreamEvent::ToolCallDelta { content_index, delta }
                if *content_index == 0 && delta == "\"README.md\"}"
        ));
        assert!(matches!(
            &events[3],
            StreamEvent::ToolCallEnd { content_index, tool_call }
                if *content_index == 0
                    && tool_call.id == "toolu_1"
                    && tool_call.name == "edit"
                    && tool_call.arguments["path"] == json!("README.md")
        ));
        assert!(matches!(
            events.last().expect("finished"),
            StreamEvent::Finished { message } if message.content.is_empty()
        ));
    }

    #[test]
    fn converts_anthropic_thinking_to_stream_events_like_pi() {
        let raw = parse_anthropic_sse_text(
            "event: message_start\n\
             data: {\"type\":\"message_start\",\"message\":{\"id\":\"msg_1\",\"usage\":{\"input_tokens\":3}}}\n\n\
             event: content_block_start\n\
             data: {\"type\":\"content_block_start\",\"index\":0,\"content_block\":{\"type\":\"thinking\",\"thinking\":\"\"}}\n\n\
             event: content_block_delta\n\
             data: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"thinking_delta\",\"thinking\":\"plan\"}}\n\n\
             event: content_block_stop\n\
             data: {\"type\":\"content_block_stop\",\"index\":0}\n\n\
             event: content_block_start\n\
             data: {\"type\":\"content_block_start\",\"index\":1,\"content_block\":{\"type\":\"text\",\"text\":\"\"}}\n\n\
             event: content_block_delta\n\
             data: {\"type\":\"content_block_delta\",\"index\":1,\"delta\":{\"type\":\"text_delta\",\"text\":\"answer\"}}\n\n\
             event: message_delta\n\
             data: {\"type\":\"message_delta\",\"delta\":{\"stop_reason\":\"end_turn\"},\"usage\":{\"output_tokens\":2}}\n\n\
             event: message_stop\n\
             data: {\"type\":\"message_stop\"}\n\n",
        )
        .expect("sse parse");
        let result = process_anthropic_sse_events(
            &raw,
            anthropic_assistant_defaults("anthropic", "anthropic-messages", "claude-haiku-4-5"),
        )
        .expect("process");

        let events = anthropic_stream_events_from_process_result(result).expect("stream events");

        assert!(matches!(
            &events[0],
            StreamEvent::ThinkingStart { content_index } if *content_index == 0
        ));
        assert!(matches!(
            &events[1],
            StreamEvent::ThinkingDelta { content_index, delta }
                if *content_index == 0 && delta == "plan"
        ));
        assert!(matches!(
            &events[2],
            StreamEvent::ThinkingEnd {
                content_index,
                content,
                ..
            } if *content_index == 0 && content == "plan"
        ));
        assert!(matches!(
            &events[3],
            StreamEvent::TextDelta { text } if text == "answer"
        ));
        assert!(matches!(
            events.last().expect("finished"),
            StreamEvent::Finished { message } if message.content == "answer"
        ));
    }

    #[test]
    fn anthropic_public_stream_preserves_thinking_signature_like_pi() {
        let events = vec![
            AnthropicRawSseEvent {
                event: "content_block_start".to_string(),
                data: serde_json::json!({
                    "type": "content_block_start",
                    "index": 0,
                    "content_block": { "type": "thinking", "thinking": "" }
                })
                .to_string(),
            },
            AnthropicRawSseEvent {
                event: "content_block_delta".to_string(),
                data: serde_json::json!({
                    "type": "content_block_delta",
                    "index": 0,
                    "delta": { "type": "thinking_delta", "thinking": "think" }
                })
                .to_string(),
            },
            AnthropicRawSseEvent {
                event: "content_block_delta".to_string(),
                data: serde_json::json!({
                    "type": "content_block_delta",
                    "index": 0,
                    "delta": { "type": "signature_delta", "signature": "sig-tail" }
                })
                .to_string(),
            },
            AnthropicRawSseEvent {
                event: "content_block_stop".to_string(),
                data: serde_json::json!({"type": "content_block_stop", "index": 0}).to_string(),
            },
            AnthropicRawSseEvent {
                event: "content_block_start".to_string(),
                data: serde_json::json!({
                    "type": "content_block_start",
                    "index": 1,
                    "content_block": { "type": "text", "text": "" }
                })
                .to_string(),
            },
            AnthropicRawSseEvent {
                event: "content_block_delta".to_string(),
                data: serde_json::json!({
                    "type": "content_block_delta",
                    "index": 1,
                    "delta": { "type": "text_delta", "text": "answer" }
                })
                .to_string(),
            },
            AnthropicRawSseEvent {
                event: "message_stop".to_string(),
                data: serde_json::json!({"type": "message_stop"}).to_string(),
            },
        ];

        let result =
            process_anthropic_sse_events(&events, empty_rich_anthropic_assistant()).expect("sse");
        let stream_events = anthropic_stream_events_from_process_result(result).expect("stream");
        let stream = crate::provider_events_to_stream(stream_events).expect("public stream");
        let message = stream.result().expect("final message");

        let thinking = message
            .content_blocks
            .iter()
            .find_map(|block| match block {
                AssistantContentBlock::Thinking(thinking) => Some(thinking),
                _ => None,
            })
            .expect("thinking block");
        assert_eq!(thinking.thinking, "think");
        assert_eq!(thinking.thinking_signature.as_deref(), Some("sig-tail"));
        assert!(!thinking.redacted);
    }

    #[test]
    fn anthropic_public_stream_preserves_redacted_thinking_like_pi() {
        let events = vec![
            AnthropicRawSseEvent {
                event: "content_block_start".to_string(),
                data: serde_json::json!({
                    "type": "content_block_start",
                    "index": 0,
                    "content_block": {
                        "type": "redacted_thinking",
                        "data": "opaque-redacted-payload"
                    }
                })
                .to_string(),
            },
            AnthropicRawSseEvent {
                event: "content_block_stop".to_string(),
                data: serde_json::json!({"type": "content_block_stop", "index": 0}).to_string(),
            },
            AnthropicRawSseEvent {
                event: "content_block_start".to_string(),
                data: serde_json::json!({
                    "type": "content_block_start",
                    "index": 1,
                    "content_block": { "type": "text", "text": "" }
                })
                .to_string(),
            },
            AnthropicRawSseEvent {
                event: "content_block_delta".to_string(),
                data: serde_json::json!({
                    "type": "content_block_delta",
                    "index": 1,
                    "delta": { "type": "text_delta", "text": "answer" }
                })
                .to_string(),
            },
            AnthropicRawSseEvent {
                event: "message_stop".to_string(),
                data: serde_json::json!({"type": "message_stop"}).to_string(),
            },
        ];

        let result =
            process_anthropic_sse_events(&events, empty_rich_anthropic_assistant()).expect("sse");
        let stream_events = anthropic_stream_events_from_process_result(result).expect("stream");
        let stream = crate::provider_events_to_stream(stream_events).expect("public stream");
        let message = stream.result().expect("final message");

        let thinking = message
            .content_blocks
            .iter()
            .find_map(|block| match block {
                AssistantContentBlock::Thinking(thinking) => Some(thinking),
                _ => None,
            })
            .expect("thinking block");
        assert_eq!(thinking.thinking, "[Reasoning redacted]");
        assert_eq!(
            thinking.thinking_signature.as_deref(),
            Some("opaque-redacted-payload")
        );
        assert!(thinking.redacted);
    }

    fn reasoning_model(id: &str) -> Model {
        Model {
            id: id.to_string(),
            provider: "anthropic".to_string(),
            api: "anthropic-messages".to_string(),
            display_name: id.to_string(),
            reasoning: Some(ModelReasoning { enabled: true }),
            ..Model::default()
        }
    }

    #[test]
    fn builds_disabled_thinking_for_reasoning_model_when_thinking_is_off_like_pi() {
        let payload = AnthropicRequest::from_stream_request(
            StreamRequest {
                model: reasoning_model("claude-sonnet-4-5"),
                messages: vec![Message {
                    role: MessageRole::User,
                    content: "Hello".to_string(),
                }],
                rich_messages: Vec::new(),
                tools: Vec::new(),
                metadata: Default::default(),
            },
            4096,
            false,
        );

        assert_eq!(payload.thinking.expect("thinking").r#type, "disabled");
        assert!(payload.output_config.is_none());
    }

    #[test]
    fn builds_adaptive_thinking_and_xhigh_effort_for_opus_four_seven_like_pi() {
        let mut model = reasoning_model("claude-opus-4-7");
        model
            .thinking_level_map
            .insert(ModelThinkingLevel::XHigh, Some("xhigh".to_string()));

        let payload = AnthropicRequest::from_stream_request(
            StreamRequest {
                model,
                messages: vec![Message {
                    role: MessageRole::User,
                    content: "Hello".to_string(),
                }],
                rich_messages: Vec::new(),
                tools: Vec::new(),
                metadata: BTreeMap::from([("reasoning".to_string(), serde_json::json!("xhigh"))]),
            },
            4096,
            false,
        );

        let thinking = payload.thinking.expect("thinking");
        assert_eq!(thinking.r#type, "adaptive");
        assert_eq!(thinking.display.as_deref(), Some("summarized"));
        assert_eq!(
            payload.output_config.expect("output config").effort,
            "xhigh"
        );
    }

    #[test]
    fn force_adaptive_thinking_compat_enables_adaptive_for_custom_model_like_pi() {
        let mut model = reasoning_model("vendor--claude-opus-latest");
        model
            .compat
            .insert("forceAdaptiveThinking".to_string(), serde_json::json!(true));

        let payload = AnthropicRequest::from_stream_request(
            StreamRequest {
                model,
                messages: vec![Message {
                    role: MessageRole::User,
                    content: "Hello".to_string(),
                }],
                rich_messages: Vec::new(),
                tools: Vec::new(),
                metadata: BTreeMap::from([("reasoning".to_string(), serde_json::json!("medium"))]),
            },
            4096,
            false,
        );

        let thinking = payload.thinking.expect("thinking");
        assert_eq!(thinking.r#type, "adaptive");
        assert_eq!(thinking.display.as_deref(), Some("summarized"));
        assert_eq!(
            payload.output_config.expect("output config").effort,
            "medium"
        );
    }

    #[test]
    fn force_adaptive_thinking_false_opts_builtin_adaptive_model_out_like_pi() {
        let mut model = reasoning_model("claude-opus-4-7");
        model.compat.insert(
            "forceAdaptiveThinking".to_string(),
            serde_json::json!(false),
        );

        let payload = AnthropicRequest::from_stream_request(
            StreamRequest {
                model,
                messages: vec![Message {
                    role: MessageRole::User,
                    content: "Hello".to_string(),
                }],
                rich_messages: Vec::new(),
                tools: Vec::new(),
                metadata: BTreeMap::from([("reasoning".to_string(), serde_json::json!("medium"))]),
            },
            4096,
            false,
        );

        let thinking = payload.thinking.expect("thinking");
        assert_eq!(thinking.r#type, "enabled");
        assert_eq!(thinking.display.as_deref(), Some("summarized"));
        assert!(payload.output_config.is_none());
    }

    #[test]
    fn builds_anthropic_tools_with_eager_input_streaming_like_pi() {
        let payload = AnthropicRequest::from_stream_request(
            StreamRequest {
                model: Model {
                    id: "claude-opus-4-7".to_string(),
                    provider: "anthropic".to_string(),
                    api: "anthropic-messages".to_string(),
                    display_name: "Claude Opus 4.7".to_string(),
                    ..Model::default()
                },
                messages: vec![Message {
                    role: MessageRole::User,
                    content: "Use the tool".to_string(),
                }],
                rich_messages: Vec::new(),
                tools: vec![crate::types::ToolDefinition {
                    name: "lookup".to_string(),
                    description: "Look up a value".to_string(),
                    parameters: serde_json::json!({
                        "type": "object",
                        "properties": {
                            "value": { "type": "string" }
                        },
                        "required": ["value"]
                    }),
                }],
                metadata: Default::default(),
            },
            4096,
            false,
        );

        let tools = payload.tools.expect("tools");
        assert_eq!(tools.len(), 1);
        assert_eq!(tools[0].name, "lookup");
        assert_eq!(tools[0].description, "Look up a value");
        assert_eq!(tools[0].eager_input_streaming, Some(true));
        assert_eq!(
            tools[0].input_schema["properties"]["value"]["type"],
            "string"
        );
        assert_eq!(tools[0].input_schema["required"][0], "value");
    }

    #[test]
    fn builds_anthropic_tools_with_cache_control_on_last_native_tool_like_pi() {
        let payload = AnthropicRequest::from_stream_request(
            StreamRequest {
                model: Model {
                    id: "claude-opus-4-7".to_string(),
                    provider: "anthropic".to_string(),
                    api: "anthropic-messages".to_string(),
                    display_name: "Claude Opus 4.7".to_string(),
                    ..Model::default()
                },
                messages: vec![Message {
                    role: MessageRole::User,
                    content: "Use the tools".to_string(),
                }],
                rich_messages: Vec::new(),
                tools: vec![
                    crate::types::ToolDefinition {
                        name: "lookup".to_string(),
                        description: "Look up a value".to_string(),
                        parameters: serde_json::json!({"type": "object"}),
                    },
                    crate::types::ToolDefinition {
                        name: "write".to_string(),
                        description: "Write a value".to_string(),
                        parameters: serde_json::json!({"type": "object"}),
                    },
                ],
                metadata: Default::default(),
            },
            4096,
            false,
        );

        let tools = payload.tools.expect("tools");
        let value = serde_json::to_value(&tools).expect("tools json");
        assert!(value[0].get("cache_control").is_none());
        assert_eq!(value[1]["cache_control"]["type"], "ephemeral");
    }

    #[test]
    fn omits_anthropic_tool_cache_control_when_retention_none_like_pi() {
        let payload = AnthropicRequest::from_stream_request(
            StreamRequest {
                model: Model {
                    id: "claude-opus-4-7".to_string(),
                    provider: "anthropic".to_string(),
                    api: "anthropic-messages".to_string(),
                    display_name: "Claude Opus 4.7".to_string(),
                    ..Model::default()
                },
                messages: vec![Message {
                    role: MessageRole::User,
                    content: "Use the tool".to_string(),
                }],
                rich_messages: Vec::new(),
                tools: vec![crate::types::ToolDefinition {
                    name: "lookup".to_string(),
                    description: "Look up a value".to_string(),
                    parameters: serde_json::json!({"type": "object"}),
                }],
                metadata: BTreeMap::from([("cacheRetention".to_string(), json!("none"))]),
            },
            4096,
            false,
        );

        let tools = payload.tools.expect("tools");
        let value = serde_json::to_value(&tools).expect("tools json");
        assert!(value[0].get("cache_control").is_none());
    }

    #[test]
    fn omits_anthropic_tool_cache_control_when_model_compat_disables_it_like_pi() {
        let mut model = Model {
            id: "accounts/fireworks/models/kimi-k2p6".to_string(),
            provider: "fireworks".to_string(),
            api: "anthropic-messages".to_string(),
            display_name: "Kimi K2.6".to_string(),
            ..Model::default()
        };
        model.compat.insert(
            "supportsCacheControlOnTools".to_string(),
            serde_json::json!(false),
        );

        let payload = AnthropicRequest::from_stream_request(
            StreamRequest {
                model,
                messages: vec![Message {
                    role: MessageRole::User,
                    content: "Use the tool".to_string(),
                }],
                rich_messages: Vec::new(),
                tools: vec![crate::types::ToolDefinition {
                    name: "lookup".to_string(),
                    description: "Look up a value".to_string(),
                    parameters: serde_json::json!({"type": "object"}),
                }],
                metadata: Default::default(),
            },
            4096,
            false,
        );

        let tools = payload.tools.expect("tools");
        let value = serde_json::to_value(&tools).expect("tools json");
        assert!(value[0].get("cache_control").is_none());
    }

    #[test]
    fn uses_long_anthropic_tool_cache_ttl_only_when_supported_like_pi() {
        let mut model = Model {
            id: "claude-opus-4-7".to_string(),
            provider: "anthropic".to_string(),
            api: "anthropic-messages".to_string(),
            display_name: "Claude Opus 4.7".to_string(),
            ..Model::default()
        };
        model.compat.insert(
            "supportsLongCacheRetention".to_string(),
            serde_json::json!(true),
        );

        let payload = AnthropicRequest::from_stream_request(
            StreamRequest {
                model: model.clone(),
                messages: vec![Message {
                    role: MessageRole::User,
                    content: "Use the tool".to_string(),
                }],
                rich_messages: Vec::new(),
                tools: vec![crate::types::ToolDefinition {
                    name: "lookup".to_string(),
                    description: "Look up a value".to_string(),
                    parameters: serde_json::json!({"type": "object"}),
                }],
                metadata: BTreeMap::from([("cacheRetention".to_string(), json!("long"))]),
            },
            4096,
            false,
        );
        let tools = payload.tools.expect("tools");
        let value = serde_json::to_value(&tools).expect("tools json");
        assert_eq!(value[0]["cache_control"]["ttl"], "1h");

        model.compat.insert(
            "supportsLongCacheRetention".to_string(),
            serde_json::json!(false),
        );
        let payload = AnthropicRequest::from_stream_request(
            StreamRequest {
                model,
                messages: vec![Message {
                    role: MessageRole::User,
                    content: "Use the tool".to_string(),
                }],
                rich_messages: Vec::new(),
                tools: vec![crate::types::ToolDefinition {
                    name: "lookup".to_string(),
                    description: "Look up a value".to_string(),
                    parameters: serde_json::json!({"type": "object"}),
                }],
                metadata: BTreeMap::from([("cacheRetention".to_string(), json!("long"))]),
            },
            4096,
            false,
        );
        let tools = payload.tools.expect("tools");
        let value = serde_json::to_value(&tools).expect("tools json");
        assert!(value[0]["cache_control"].get("ttl").is_none());
    }

    #[test]
    fn builds_legacy_fine_grained_beta_when_eager_tool_input_is_disabled_like_pi() {
        let mut model = Model {
            id: "claude-opus-4-7".to_string(),
            provider: "anthropic".to_string(),
            api: "anthropic-messages".to_string(),
            display_name: "Claude Opus 4.7".to_string(),
            ..Model::default()
        };
        model.compat.insert(
            "supportsEagerToolInputStreaming".to_string(),
            serde_json::json!(false),
        );
        let request = StreamRequest {
            model,
            messages: vec![Message {
                role: MessageRole::User,
                content: "Use the tool".to_string(),
            }],
            rich_messages: Vec::new(),
            tools: vec![crate::types::ToolDefinition {
                name: "lookup".to_string(),
                description: "Look up a value".to_string(),
                parameters: serde_json::json!({"type": "object"}),
            }],
            metadata: Default::default(),
        };

        assert_eq!(
            anthropic_beta_header_for_request(&request),
            Some("fine-grained-tool-streaming-2025-05-14")
        );
        let payload = AnthropicRequest::from_stream_request(request, 4096, false);
        let tools = payload.tools.expect("tools");
        assert_eq!(tools[0].eager_input_streaming, None);
    }

    #[test]
    fn omits_legacy_fine_grained_beta_when_no_tools_like_pi() {
        let mut model = Model {
            id: "claude-opus-4-7".to_string(),
            provider: "anthropic".to_string(),
            api: "anthropic-messages".to_string(),
            display_name: "Claude Opus 4.7".to_string(),
            ..Model::default()
        };
        model.compat.insert(
            "supportsEagerToolInputStreaming".to_string(),
            serde_json::json!(false),
        );
        let request = StreamRequest {
            model,
            messages: vec![Message {
                role: MessageRole::User,
                content: "Hello".to_string(),
            }],
            rich_messages: Vec::new(),
            tools: Vec::new(),
            metadata: Default::default(),
        };

        assert_eq!(anthropic_beta_header_for_request(&request), None);
        let payload = AnthropicRequest::from_stream_request(request, 4096, false);
        assert!(payload.tools.is_none());
    }

    #[test]
    fn normalizes_anthropic_oauth_tool_names_to_claude_code_casing_like_pi() {
        let tools = vec![
            crate::types::ToolDefinition {
                name: "todowrite".to_string(),
                description: "Write todos".to_string(),
                parameters: serde_json::json!({"type": "object"}),
            },
            crate::types::ToolDefinition {
                name: "read".to_string(),
                description: "Read files".to_string(),
                parameters: serde_json::json!({"type": "object"}),
            },
            crate::types::ToolDefinition {
                name: "find".to_string(),
                description: "Find files".to_string(),
                parameters: serde_json::json!({"type": "object"}),
            },
            crate::types::ToolDefinition {
                name: "my_custom_tool".to_string(),
                description: "Custom".to_string(),
                parameters: serde_json::json!({"type": "object"}),
            },
        ];

        let converted = convert_anthropic_tools(&tools, true, true, None).expect("tools");

        assert_eq!(converted[0].name, "TodoWrite");
        assert_eq!(converted[1].name, "Read");
        assert_eq!(converted[2].name, "find");
        assert_eq!(converted[3].name, "my_custom_tool");
    }

    #[test]
    fn preserves_anthropic_tool_use_start_input_like_pi() {
        let events = vec![
            AnthropicRawSseEvent {
                event: "content_block_start".to_string(),
                data: serde_json::json!({
                    "type": "content_block_start",
                    "index": 0,
                    "content_block": {
                        "type": "tool_use",
                        "id": "toolu_start",
                        "name": "edit",
                        "input": {
                            "path": "README.md",
                            "limit": 5
                        }
                    }
                })
                .to_string(),
            },
            AnthropicRawSseEvent {
                event: "content_block_stop".to_string(),
                data: serde_json::json!({
                    "type": "content_block_stop",
                    "index": 0
                })
                .to_string(),
            },
        ];

        let result =
            process_anthropic_sse_events(&events, empty_rich_anthropic_assistant()).expect("sse");
        let tool_call = result
            .assistant
            .content
            .iter()
            .find_map(|block| match block {
                AssistantContentBlock::ToolCall(tool_call) => Some(tool_call),
                _ => None,
            })
            .expect("tool call");

        assert_eq!(tool_call.arguments["path"], json!("README.md"));
        assert_eq!(tool_call.arguments["limit"], json!(5));
    }

    fn empty_rich_anthropic_assistant() -> RichAssistantMessage {
        RichAssistantMessage {
            content: Vec::new(),
            api: "anthropic-messages".to_string(),
            provider: "anthropic".to_string(),
            model: "claude-haiku-4-5".to_string(),
            response_model: None,
            response_id: None,
            usage: Default::default(),
            stop_reason: crate::types::AssistantStopReason::Stop,
            error_message: None,
            diagnostics: Vec::new(),
            timestamp_millis: 0,
        }
    }

    #[test]
    fn processes_anthropic_sse_tool_json_with_repair_like_pi() {
        let malformed_tool_json_delta = r#"{"type":"content_block_delta","index":0,"delta":{"type":"input_json_delta","partial_json":"{\"path\":\"A\H\",\"text\":\"col1	col2\"}"}}"#;
        let events = vec![
            AnthropicRawSseEvent {
                event: "message_start".to_string(),
                data: serde_json::json!({
                    "type": "message_start",
                    "message": {
                        "id": "msg_test",
                        "usage": {
                            "input_tokens": 12,
                            "output_tokens": 0,
                            "cache_read_input_tokens": 0,
                            "cache_creation_input_tokens": 0
                        }
                    }
                })
                .to_string(),
            },
            AnthropicRawSseEvent {
                event: "content_block_start".to_string(),
                data: serde_json::json!({
                    "type": "content_block_start",
                    "index": 0,
                    "content_block": {
                        "type": "tool_use",
                        "id": "toolu_test",
                        "name": "edit",
                        "input": {}
                    }
                })
                .to_string(),
            },
            AnthropicRawSseEvent {
                event: "content_block_delta".to_string(),
                data: malformed_tool_json_delta.to_string(),
            },
            AnthropicRawSseEvent {
                event: "content_block_stop".to_string(),
                data: serde_json::json!({"type": "content_block_stop", "index": 0}).to_string(),
            },
            AnthropicRawSseEvent {
                event: "message_delta".to_string(),
                data: serde_json::json!({
                    "type": "message_delta",
                    "delta": { "stop_reason": "tool_use" },
                    "usage": {
                        "input_tokens": 12,
                        "output_tokens": 5,
                        "cache_read_input_tokens": 0,
                        "cache_creation_input_tokens": 0
                    }
                })
                .to_string(),
            },
            AnthropicRawSseEvent {
                event: "message_stop".to_string(),
                data: serde_json::json!({"type": "message_stop"}).to_string(),
            },
        ];

        let result =
            process_anthropic_sse_events(&events, empty_rich_anthropic_assistant()).expect("sse");

        assert_eq!(
            result.assistant.stop_reason,
            crate::types::AssistantStopReason::ToolUse
        );
        let tool_call = result
            .assistant
            .content
            .iter()
            .find_map(|block| match block {
                AssistantContentBlock::ToolCall(tool_call) => Some(tool_call),
                _ => None,
            })
            .expect("tool call");
        assert_eq!(tool_call.id, "toolu_test");
        assert_eq!(tool_call.name, "edit");
        assert_eq!(tool_call.arguments["path"], "A\\H");
        assert_eq!(tool_call.arguments["text"], "col1\tcol2");
    }

    #[test]
    fn normalizes_anthropic_oauth_inbound_tool_names_back_to_context_tools_like_pi() {
        let events = vec![
            AnthropicRawSseEvent {
                event: "content_block_start".to_string(),
                data: serde_json::json!({
                    "type": "content_block_start",
                    "index": 0,
                    "content_block": {
                        "type": "tool_use",
                        "id": "toolu_todo",
                        "name": "TodoWrite",
                        "input": {}
                    }
                })
                .to_string(),
            },
            AnthropicRawSseEvent {
                event: "content_block_start".to_string(),
                data: serde_json::json!({
                    "type": "content_block_start",
                    "index": 1,
                    "content_block": {
                        "type": "tool_use",
                        "id": "toolu_glob",
                        "name": "Glob",
                        "input": {}
                    }
                })
                .to_string(),
            },
        ];
        let options = AnthropicSseProcessOptions {
            is_oauth_token: true,
            tools: vec![
                crate::types::ToolDefinition {
                    name: "todowrite".to_string(),
                    description: "Write todos".to_string(),
                    parameters: serde_json::json!({"type": "object"}),
                },
                crate::types::ToolDefinition {
                    name: "find".to_string(),
                    description: "Find files".to_string(),
                    parameters: serde_json::json!({"type": "object"}),
                },
            ],
        };

        let result = process_anthropic_sse_events_with_options(
            &events,
            empty_rich_anthropic_assistant(),
            &options,
        )
        .expect("sse");

        let tool_names = result
            .assistant
            .content
            .iter()
            .filter_map(|block| match block {
                AssistantContentBlock::ToolCall(tool_call) => Some(tool_call.name.as_str()),
                _ => None,
            })
            .collect::<Vec<_>>();
        assert_eq!(tool_names, vec!["todowrite", "Glob"]);
    }

    #[test]
    fn computes_anthropic_sse_usage_total_with_cache_tokens_like_pi() {
        let events = vec![
            AnthropicRawSseEvent {
                event: "message_start".to_string(),
                data: serde_json::json!({
                    "type": "message_start",
                    "message": {
                        "id": "msg_usage",
                        "usage": {
                            "input_tokens": 12,
                            "output_tokens": 5,
                            "cache_read_input_tokens": 7,
                            "cache_creation_input_tokens": 11
                        }
                    }
                })
                .to_string(),
            },
            AnthropicRawSseEvent {
                event: "message_stop".to_string(),
                data: serde_json::json!({ "type": "message_stop" }).to_string(),
            },
        ];

        let result =
            process_anthropic_sse_events(&events, empty_rich_anthropic_assistant()).expect("sse");

        assert_eq!(result.assistant.response_id.as_deref(), Some("msg_usage"));
        assert_eq!(result.assistant.usage.input, 12);
        assert_eq!(result.assistant.usage.output, 5);
        assert_eq!(result.assistant.usage.cache_read, 7);
        assert_eq!(result.assistant.usage.cache_write, 11);
        assert_eq!(result.assistant.usage.total_tokens, 35);
    }

    #[test]
    fn merges_anthropic_message_delta_usage_fields_like_pi() {
        let events = vec![
            AnthropicRawSseEvent {
                event: "message_start".to_string(),
                data: serde_json::json!({
                    "type": "message_start",
                    "message": {
                        "id": "msg_usage",
                        "usage": {
                            "input_tokens": 12,
                            "output_tokens": 0,
                            "cache_read_input_tokens": 7,
                            "cache_creation_input_tokens": 11
                        }
                    }
                })
                .to_string(),
            },
            AnthropicRawSseEvent {
                event: "message_delta".to_string(),
                data: serde_json::json!({
                    "type": "message_delta",
                    "delta": { "stop_reason": "end_turn" },
                    "usage": {
                        "output_tokens": 5
                    }
                })
                .to_string(),
            },
            AnthropicRawSseEvent {
                event: "message_stop".to_string(),
                data: serde_json::json!({ "type": "message_stop" }).to_string(),
            },
        ];

        let result =
            process_anthropic_sse_events(&events, empty_rich_anthropic_assistant()).expect("sse");

        assert_eq!(result.assistant.usage.input, 12);
        assert_eq!(result.assistant.usage.output, 5);
        assert_eq!(result.assistant.usage.cache_read, 7);
        assert_eq!(result.assistant.usage.cache_write, 11);
        assert_eq!(result.assistant.usage.total_tokens, 35);
    }

    #[test]
    fn maps_anthropic_refusal_stop_reason_to_error_like_pi() {
        let events = vec![AnthropicRawSseEvent {
            event: "message_delta".to_string(),
            data: serde_json::json!({
                "type": "message_delta",
                "delta": { "stop_reason": "refusal" },
                "usage": {}
            })
            .to_string(),
        }];

        let result =
            process_anthropic_sse_events(&events, empty_rich_anthropic_assistant()).expect("sse");

        assert_eq!(
            result.assistant.stop_reason,
            crate::types::AssistantStopReason::Error
        );
    }

    #[test]
    fn errors_on_unknown_anthropic_stop_reason_like_pi() {
        let events = vec![AnthropicRawSseEvent {
            event: "message_delta".to_string(),
            data: serde_json::json!({
                "type": "message_delta",
                "delta": { "stop_reason": "new_future_reason" },
                "usage": {}
            })
            .to_string(),
        }];

        let error = process_anthropic_sse_events(&events, empty_rich_anthropic_assistant())
            .expect_err("unknown stop reason should error");

        assert!(error.contains("Unhandled stop reason: new_future_reason"));
    }

    #[test]
    fn errors_on_anthropic_sse_error_event_like_pi() {
        let events = vec![AnthropicRawSseEvent {
            event: "error".to_string(),
            data: "overloaded_error".to_string(),
        }];

        let error = process_anthropic_sse_events(&events, empty_rich_anthropic_assistant())
            .expect_err("error event should stop processing");

        assert_eq!(error, "overloaded_error");
    }

    #[test]
    fn errors_when_anthropic_stream_ends_before_message_stop_like_pi() {
        let events = vec![AnthropicRawSseEvent {
            event: "message_start".to_string(),
            data: serde_json::json!({
                "type": "message_start",
                "message": {
                    "id": "msg_incomplete",
                    "usage": { "input_tokens": 1, "output_tokens": 0 }
                }
            })
            .to_string(),
        }];

        let error = process_anthropic_sse_events(&events, empty_rich_anthropic_assistant())
            .expect_err("missing message_stop should error");

        assert_eq!(error, "Anthropic stream ended before message_stop");
    }

    #[test]
    fn processes_anthropic_sse_thinking_and_signature_like_pi() {
        let events = vec![
            AnthropicRawSseEvent {
                event: "content_block_start".to_string(),
                data: serde_json::json!({
                    "type": "content_block_start",
                    "index": 0,
                    "content_block": { "type": "thinking", "thinking": "" }
                })
                .to_string(),
            },
            AnthropicRawSseEvent {
                event: "content_block_delta".to_string(),
                data: serde_json::json!({
                    "type": "content_block_delta",
                    "index": 0,
                    "delta": { "type": "thinking_delta", "thinking": "think" }
                })
                .to_string(),
            },
            AnthropicRawSseEvent {
                event: "content_block_delta".to_string(),
                data: serde_json::json!({
                    "type": "content_block_delta",
                    "index": 0,
                    "delta": { "type": "signature_delta", "signature": "sig-" }
                })
                .to_string(),
            },
            AnthropicRawSseEvent {
                event: "content_block_delta".to_string(),
                data: serde_json::json!({
                    "type": "content_block_delta",
                    "index": 0,
                    "delta": { "type": "signature_delta", "signature": "tail" }
                })
                .to_string(),
            },
            AnthropicRawSseEvent {
                event: "content_block_stop".to_string(),
                data: serde_json::json!({"type": "content_block_stop", "index": 0}).to_string(),
            },
        ];

        let result =
            process_anthropic_sse_events(&events, empty_rich_anthropic_assistant()).expect("sse");

        let thinking = result
            .assistant
            .content
            .iter()
            .find_map(|block| match block {
                AssistantContentBlock::Thinking(thinking) => Some(thinking),
                _ => None,
            })
            .expect("thinking block");
        assert_eq!(thinking.thinking, "think");
        assert_eq!(thinking.thinking_signature.as_deref(), Some("sig-tail"));
        assert!(!thinking.redacted);
    }

    #[test]
    fn processes_anthropic_redacted_thinking_sse_like_pi() {
        let events = vec![
            AnthropicRawSseEvent {
                event: "content_block_start".to_string(),
                data: serde_json::json!({
                    "type": "content_block_start",
                    "index": 0,
                    "content_block": {
                        "type": "redacted_thinking",
                        "data": "opaque-redacted-payload"
                    }
                })
                .to_string(),
            },
            AnthropicRawSseEvent {
                event: "content_block_stop".to_string(),
                data: serde_json::json!({"type": "content_block_stop", "index": 0}).to_string(),
            },
        ];

        let result =
            process_anthropic_sse_events(&events, empty_rich_anthropic_assistant()).expect("sse");

        let thinking = result
            .assistant
            .content
            .iter()
            .find_map(|block| match block {
                AssistantContentBlock::Thinking(thinking) => Some(thinking),
                _ => None,
            })
            .expect("thinking block");
        assert_eq!(thinking.thinking, "[Reasoning redacted]");
        assert_eq!(
            thinking.thinking_signature.as_deref(),
            Some("opaque-redacted-payload")
        );
        assert!(thinking.redacted);
    }

    #[test]
    fn ignores_unknown_anthropic_sse_events_after_message_stop_like_pi() {
        let events = vec![
            AnthropicRawSseEvent {
                event: "message_start".to_string(),
                data: serde_json::json!({
                    "type": "message_start",
                    "message": {
                        "id": "msg_test",
                        "usage": {
                            "input_tokens": 12,
                            "output_tokens": 0,
                            "cache_read_input_tokens": 0,
                            "cache_creation_input_tokens": 0
                        }
                    }
                })
                .to_string(),
            },
            AnthropicRawSseEvent {
                event: "content_block_start".to_string(),
                data: serde_json::json!({
                    "type": "content_block_start",
                    "index": 0,
                    "content_block": { "type": "text", "text": "" }
                })
                .to_string(),
            },
            AnthropicRawSseEvent {
                event: "content_block_delta".to_string(),
                data: serde_json::json!({
                    "type": "content_block_delta",
                    "index": 0,
                    "delta": { "type": "text_delta", "text": "Hello" }
                })
                .to_string(),
            },
            AnthropicRawSseEvent {
                event: "content_block_stop".to_string(),
                data: serde_json::json!({"type": "content_block_stop", "index": 0}).to_string(),
            },
            AnthropicRawSseEvent {
                event: "message_delta".to_string(),
                data: serde_json::json!({
                    "type": "message_delta",
                    "delta": { "stop_reason": "end_turn" },
                    "usage": {
                        "input_tokens": 12,
                        "output_tokens": 5,
                        "cache_read_input_tokens": 0,
                        "cache_creation_input_tokens": 0
                    }
                })
                .to_string(),
            },
            AnthropicRawSseEvent {
                event: "message_stop".to_string(),
                data: serde_json::json!({"type": "message_stop"}).to_string(),
            },
            AnthropicRawSseEvent {
                event: "done".to_string(),
                data: "[DONE]".to_string(),
            },
            AnthropicRawSseEvent {
                event: "proxy.stats".to_string(),
                data: "not json".to_string(),
            },
        ];

        let result =
            process_anthropic_sse_events(&events, empty_rich_anthropic_assistant()).expect("sse");

        assert_eq!(
            result.assistant.stop_reason,
            crate::types::AssistantStopReason::Stop
        );
        assert_eq!(result.assistant.error_message, None);
        assert_eq!(
            result.assistant.content,
            vec![AssistantContentBlock::Text(
                crate::conversation::TextContent {
                    text: "Hello".to_string(),
                    text_signature: None,
                }
            )]
        );
    }
}
