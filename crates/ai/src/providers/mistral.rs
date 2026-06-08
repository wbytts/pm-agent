use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::env;

use crate::conversation::{AssistantContentBlock, RichAssistantMessage, TextContent};
use crate::providers::chat_role;
use crate::types::{
    validate_model, AiError, AiResult, AssistantStopReason, LanguageModelProvider,
    ModelThinkingLevel, StreamEvent, StreamRequest, StreamToolCall, ToolDefinition, Usage,
};
use crate::utils::parse_streaming_json;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MistralChatConfig {
    pub api_key: Option<String>,
    pub base_url: String,
}

#[derive(Debug, Clone)]
pub struct MistralChatProvider {
    config: MistralChatConfig,
}

impl MistralChatProvider {
    pub fn new(config: MistralChatConfig) -> Self {
        Self { config }
    }

    pub fn from_env() -> Self {
        Self::new(MistralChatConfig {
            api_key: env::var("MISTRAL_API_KEY").ok(),
            base_url: env::var("MISTRAL_BASE_URL")
                .unwrap_or_else(|_| "https://api.mistral.ai/v1".to_string()),
        })
    }

    fn api_key(&self) -> AiResult<String> {
        let key = self.config.api_key.as_deref().unwrap_or_default().trim();
        if key.is_empty() {
            return Err(AiError::MissingApiKey("MISTRAL_API_KEY".to_string()));
        }
        Ok(key.to_string())
    }
}

impl LanguageModelProvider for MistralChatProvider {
    fn stream(&self, request: StreamRequest) -> AiResult<Vec<StreamEvent>> {
        validate_model(&request.model)?;
        let api_key = self.api_key()?;
        let base_url = request
            .model
            .base_url
            .as_deref()
            .unwrap_or(&self.config.base_url);
        let url = format!("{}/chat/completions", base_url.trim_end_matches('/'));
        let payload = MistralChatRequest::from_stream_request(request);

        let response = reqwest::blocking::Client::new()
            .post(url)
            .bearer_auth(api_key)
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
        mistral_sse_text_to_stream_events(&body).map_err(AiError::InvalidResponse)
    }
}

#[derive(Debug, Serialize)]
struct MistralChatRequest {
    model: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    stream: Option<bool>,
    messages: Vec<MistralChatMessage>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tools: Option<Vec<MistralToolDefinition>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    prompt_mode: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    reasoning_effort: Option<String>,
}

impl MistralChatRequest {
    fn new(
        model: String,
        messages: Vec<MistralChatMessage>,
        tools: Vec<ToolDefinition>,
        reasoning: Option<ModelThinkingLevel>,
    ) -> Self {
        let (prompt_mode, reasoning_effort) =
            mistral_reasoning_controls(&model, reasoning).unwrap_or((None, None));
        Self {
            model,
            stream: Some(true),
            messages,
            tools: convert_mistral_tools(&tools),
            prompt_mode,
            reasoning_effort,
        }
    }

    fn from_stream_request(request: StreamRequest) -> Self {
        let reasoning = request.metadata.get("reasoning").and_then(parse_reasoning);
        let messages = request
            .messages
            .iter()
            .map(|message| MistralChatMessage {
                role: chat_role(&message.role).to_string(),
                content: message.content.clone(),
            })
            .collect::<Vec<_>>();
        Self::new(request.model.id, messages, request.tools, reasoning)
    }
}

fn convert_mistral_tools(tools: &[ToolDefinition]) -> Option<Vec<MistralToolDefinition>> {
    if tools.is_empty() {
        return None;
    }
    Some(
        tools
            .iter()
            .map(|tool| MistralToolDefinition {
                r#type: "function".to_string(),
                function: MistralToolFunction {
                    name: tool.name.clone(),
                    description: tool.description.clone(),
                    parameters: tool.parameters.clone(),
                },
            })
            .collect(),
    )
}

fn parse_reasoning(value: &Value) -> Option<ModelThinkingLevel> {
    match value.as_str()? {
        "minimal" => Some(ModelThinkingLevel::Minimal),
        "low" => Some(ModelThinkingLevel::Low),
        "medium" => Some(ModelThinkingLevel::Medium),
        "high" => Some(ModelThinkingLevel::High),
        "xhigh" => Some(ModelThinkingLevel::XHigh),
        _ => None,
    }
}

fn mistral_reasoning_controls(
    model: &str,
    reasoning: Option<ModelThinkingLevel>,
) -> Option<(Option<String>, Option<String>)> {
    reasoning?;
    if model.starts_with("magistral-") {
        return Some((Some("reasoning".to_string()), None));
    }
    if model.starts_with("mistral-small-") || model.starts_with("mistral-medium-3.5") {
        return Some((None, Some("high".to_string())));
    }
    None
}

#[derive(Debug, Serialize)]
struct MistralChatMessage {
    role: String,
    content: String,
}

#[derive(Debug, Serialize)]
struct MistralToolDefinition {
    r#type: String,
    function: MistralToolFunction,
}

#[derive(Debug, Serialize)]
struct MistralToolFunction {
    name: String,
    description: String,
    parameters: Value,
}

#[derive(Debug, Deserialize)]
struct MistralStreamChunk {
    id: Option<String>,
    choices: Vec<MistralStreamChoice>,
    usage: Option<MistralStreamUsage>,
}

#[derive(Debug, Deserialize)]
struct MistralStreamChoice {
    delta: Option<MistralStreamDelta>,
    finish_reason: Option<String>,
}

#[derive(Debug, Deserialize)]
struct MistralStreamDelta {
    content: Option<Value>,
    #[serde(default, alias = "toolCalls")]
    tool_calls: Vec<MistralStreamToolCallDelta>,
}

#[derive(Debug, Deserialize)]
struct MistralStreamToolCallDelta {
    index: Option<usize>,
    id: Option<String>,
    function: MistralStreamToolCallFunctionDelta,
}

#[derive(Debug, Deserialize)]
struct MistralStreamToolCallFunctionDelta {
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    arguments: Option<Value>,
}

#[derive(Debug, Deserialize)]
struct MistralStreamUsage {
    #[serde(alias = "promptTokens")]
    prompt_tokens: Option<u64>,
    #[serde(alias = "completionTokens")]
    completion_tokens: Option<u64>,
    #[serde(alias = "totalTokens")]
    total_tokens: Option<u64>,
}

fn mistral_sse_text_to_stream_events(input: &str) -> Result<Vec<StreamEvent>, String> {
    let chunks = parse_mistral_sse_text(input)?;
    let mut events = Vec::new();
    let mut content = String::new();
    let mut saw_finish = false;
    let mut response_id = None;
    let mut stop_reason = AssistantStopReason::Stop;
    let mut next_content_index = 0usize;
    let mut current_text_open = false;
    let mut tool_states = Vec::<MistralToolCallStreamState>::new();

    for chunk in chunks {
        if response_id.is_none() {
            response_id = chunk.id.clone().filter(|value| !value.is_empty());
        }
        if let Some(choice) = chunk.choices.into_iter().next() {
            if let Some(reason) = choice.finish_reason.as_deref() {
                saw_finish = true;
                stop_reason = map_mistral_chat_stop_reason(reason);
            }
            if let Some(delta) = choice.delta {
                process_mistral_delta_content(
                    delta.content,
                    &mut events,
                    &mut content,
                    &mut next_content_index,
                    &mut current_text_open,
                )?;
                for tool_call in delta.tool_calls {
                    current_text_open = false;
                    let state_index = ensure_mistral_tool_call_state(
                        &mut tool_states,
                        &mut next_content_index,
                        &tool_call,
                    );
                    let state = &mut tool_states[state_index];
                    if let Some(name) = tool_call.function.name.filter(|name| !name.is_empty()) {
                        state.name = name;
                    }
                    if state.partial_arguments.is_empty() {
                        events.push(StreamEvent::ToolCallStart {
                            content_index: state.content_index,
                        });
                    }
                    let arguments_delta =
                        mistral_tool_arguments_delta(tool_call.function.arguments.as_ref());
                    if !arguments_delta.is_empty() {
                        state.partial_arguments.push_str(&arguments_delta);
                        events.push(StreamEvent::ToolCallDelta {
                            content_index: state.content_index,
                            delta: arguments_delta,
                        });
                    }
                    stop_reason = AssistantStopReason::ToolUse;
                }
            }
        }

        if let Some(usage) = chunk.usage {
            events.push(StreamEvent::Usage {
                usage: Usage {
                    input: usage.prompt_tokens.unwrap_or_default(),
                    output: usage.completion_tokens.unwrap_or_default(),
                    cache_read: 0,
                    cache_write: 0,
                    total_tokens: usage.total_tokens.unwrap_or_else(|| {
                        usage.prompt_tokens.unwrap_or_default()
                            + usage.completion_tokens.unwrap_or_default()
                    }),
                    cost: Default::default(),
                },
            });
        }
    }

    for state in &tool_states {
        events.push(StreamEvent::ToolCallEnd {
            content_index: state.content_index,
            tool_call: StreamToolCall {
                id: state.id.clone(),
                name: state.name.clone(),
                arguments: parse_mistral_tool_arguments(&state.partial_arguments),
                thought_signature: None,
            },
        });
    }

    if content.is_empty() && tool_states.is_empty() {
        return Err("Mistral 输出文本缺失".to_string());
    }
    if !saw_finish {
        return Err("Mistral stream ended without finish_reason".to_string());
    }
    events.push(StreamEvent::RichFinished {
        message: RichAssistantMessage {
            content: vec![AssistantContentBlock::Text(TextContent {
                text: content,
                text_signature: None,
            })],
            api: "mistral".to_string(),
            provider: "mistral".to_string(),
            model: String::new(),
            response_model: None,
            response_id,
            usage: Usage::default(),
            stop_reason,
            error_message: None,
            diagnostics: Vec::new(),
            timestamp_millis: 0,
        },
    });
    Ok(events)
}

#[derive(Debug)]
struct MistralToolCallStreamState {
    stream_index: Option<usize>,
    id: String,
    name: String,
    content_index: usize,
    partial_arguments: String,
}

fn map_mistral_chat_stop_reason(reason: &str) -> AssistantStopReason {
    match reason {
        "stop" => AssistantStopReason::Stop,
        "length" | "model_length" => AssistantStopReason::Length,
        "tool_calls" => AssistantStopReason::ToolUse,
        "error" => AssistantStopReason::Error,
        _ => AssistantStopReason::Stop,
    }
}

fn ensure_mistral_tool_call_state(
    tool_states: &mut Vec<MistralToolCallStreamState>,
    next_content_index: &mut usize,
    tool_call: &MistralStreamToolCallDelta,
) -> usize {
    if let Some(index) = tool_call.index {
        if let Some((position, _)) = tool_states
            .iter()
            .enumerate()
            .find(|(_, state)| state.stream_index == Some(index))
        {
            return position;
        }
    }
    if let Some(id) = tool_call
        .id
        .as_deref()
        .filter(|id| !id.is_empty() && *id != "null")
    {
        if let Some((position, _)) = tool_states
            .iter()
            .enumerate()
            .find(|(_, state)| state.id == id)
        {
            return position;
        }
    }

    let id = tool_call
        .id
        .as_deref()
        .filter(|id| !id.is_empty() && *id != "null")
        .map(str::to_string)
        .unwrap_or_else(|| format!("toolcall_{}", tool_call.index.unwrap_or(tool_states.len())));
    let name = tool_call.function.name.clone().unwrap_or_default();
    let content_index = *next_content_index;
    *next_content_index += 1;
    tool_states.push(MistralToolCallStreamState {
        stream_index: tool_call.index,
        id,
        name,
        content_index,
        partial_arguments: String::new(),
    });
    tool_states.len() - 1
}

fn mistral_tool_arguments_delta(arguments: Option<&Value>) -> String {
    match arguments {
        Some(Value::String(value)) => value.clone(),
        Some(value) => serde_json::to_string(value).unwrap_or_default(),
        None => String::new(),
    }
}

fn parse_mistral_tool_arguments(arguments: &str) -> std::collections::BTreeMap<String, Value> {
    match parse_streaming_json(Some(arguments)) {
        Value::Object(map) => map.into_iter().collect(),
        _ => Default::default(),
    }
}

fn parse_mistral_sse_text(input: &str) -> Result<Vec<MistralStreamChunk>, String> {
    let mut chunks = Vec::new();
    let mut data_lines = Vec::<String>::new();

    for line in input.lines().chain(std::iter::once("")) {
        let line = line.trim_end_matches('\r');
        if line.is_empty() {
            if !data_lines.is_empty() {
                let data = data_lines.join("\n").trim().to_string();
                data_lines.clear();
                if !data.is_empty() && data != "[DONE]" {
                    chunks.push(
                        serde_json::from_str::<MistralStreamChunk>(&data)
                            .map_err(|error| format!("Mistral SSE JSON 无效：{error}"))?,
                    );
                }
            }
            continue;
        }
        if let Some(value) = line.strip_prefix("data:") {
            data_lines.push(value.trim().to_string());
        }
    }

    Ok(chunks)
}

fn process_mistral_delta_content(
    content: Option<Value>,
    events: &mut Vec<StreamEvent>,
    text_content: &mut String,
    next_content_index: &mut usize,
    current_text_open: &mut bool,
) -> Result<(), String> {
    match content {
        Some(Value::String(text)) => push_mistral_text_delta(
            events,
            text_content,
            next_content_index,
            current_text_open,
            text,
        ),
        Some(Value::Array(items)) => {
            for item in items {
                match item {
                    Value::String(text) => {
                        push_mistral_text_delta(
                            events,
                            text_content,
                            next_content_index,
                            current_text_open,
                            text,
                        )?;
                    }
                    Value::Object(mut object) => match object.get("type").and_then(Value::as_str) {
                        Some("thinking") => {
                            let thinking = mistral_thinking_text(object.remove("thinking"));
                            if thinking.is_empty() {
                                continue;
                            }
                            *current_text_open = false;
                            let content_index = *next_content_index;
                            *next_content_index += 1;
                            events.push(StreamEvent::ThinkingStart { content_index });
                            events.push(StreamEvent::ThinkingDelta {
                                content_index,
                                delta: thinking.clone(),
                            });
                            events.push(StreamEvent::ThinkingEnd {
                                content_index,
                                content: thinking,
                                thinking_signature: None,
                                redacted: false,
                            });
                        }
                        _ => {
                            if let Some(text) = object
                                .remove("text")
                                .and_then(|value| value.as_str().map(str::to_string))
                            {
                                push_mistral_text_delta(
                                    events,
                                    text_content,
                                    next_content_index,
                                    current_text_open,
                                    text,
                                )?;
                            }
                        }
                    },
                    _ => {}
                }
            }
            Ok(())
        }
        _ => Ok(()),
    }
}

fn push_mistral_text_delta(
    events: &mut Vec<StreamEvent>,
    text_content: &mut String,
    next_content_index: &mut usize,
    current_text_open: &mut bool,
    text: String,
) -> Result<(), String> {
    if text.is_empty() {
        return Ok(());
    }
    if !*current_text_open {
        *next_content_index += 1;
        *current_text_open = true;
    }
    text_content.push_str(&text);
    events.push(StreamEvent::TextDelta { text });
    Ok(())
}

fn mistral_thinking_text(value: Option<Value>) -> String {
    match value {
        Some(Value::String(text)) => text,
        Some(Value::Array(items)) => items
            .into_iter()
            .filter_map(|item| match item {
                Value::String(text) => Some(text),
                Value::Object(mut object) => object
                    .remove("text")
                    .and_then(|value| value.as_str().map(str::to_string)),
                _ => None,
            })
            .collect(),
        _ => String::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::MessageRole;

    fn user_message() -> MistralChatMessage {
        MistralChatMessage {
            role: "user".to_string(),
            content: "Hello".to_string(),
        }
    }

    #[test]
    fn builds_reasoning_effort_for_mistral_small_four_like_pi() {
        let request = MistralChatRequest::new(
            "mistral-small-2603".to_string(),
            vec![user_message()],
            Vec::new(),
            Some(ModelThinkingLevel::Medium),
        );

        assert_eq!(request.reasoning_effort.as_deref(), Some("high"));
        assert_eq!(request.prompt_mode, None);
    }

    #[test]
    fn builds_prompt_mode_for_magistral_reasoning_models_like_pi() {
        let request = MistralChatRequest::new(
            "magistral-medium-latest".to_string(),
            vec![user_message()],
            Vec::new(),
            Some(ModelThinkingLevel::Medium),
        );

        assert_eq!(request.prompt_mode.as_deref(), Some("reasoning"));
        assert_eq!(request.reasoning_effort, None);
    }

    #[test]
    fn omits_mistral_reasoning_controls_when_thinking_is_off_like_pi() {
        let request = MistralChatRequest::new(
            "mistral-medium-3.5".to_string(),
            vec![user_message()],
            Vec::new(),
            None,
        );

        assert_eq!(request.prompt_mode, None);
        assert_eq!(request.reasoning_effort, None);
    }

    #[test]
    fn builds_reasoning_payload_from_stream_request_metadata_like_pi() {
        let request = StreamRequest {
            model: crate::types::Model {
                id: "mistral-medium-3.5".to_string(),
                provider: "mistral".to_string(),
                api: "mistral-chat-completions".to_string(),
                display_name: "Mistral Medium 3.5".to_string(),
                ..crate::types::Model::default()
            },
            messages: vec![crate::types::Message {
                role: MessageRole::User,
                content: "Hello".to_string(),
            }],
            rich_messages: Vec::new(),
            tools: Vec::new(),
            metadata: std::collections::BTreeMap::from([(
                "reasoning".to_string(),
                serde_json::json!("medium"),
            )]),
        };

        let payload = MistralChatRequest::from_stream_request(request);

        assert_eq!(payload.reasoning_effort.as_deref(), Some("high"));
        assert_eq!(payload.prompt_mode, None);
    }

    #[test]
    fn builds_streaming_mistral_chat_payload_like_pi() {
        let request = MistralChatRequest::new(
            "mistral-small-2603".to_string(),
            vec![user_message()],
            Vec::new(),
            None,
        );

        assert_eq!(request.stream, Some(true));
    }

    #[test]
    fn parses_mistral_sse_text_to_public_stream_events_like_pi() {
        let sse = "data: {\"id\":\"cmpl_1\",\"choices\":[{\"delta\":{\"content\":\"hel\"},\"finish_reason\":null}]}\n\n\
                   data: {\"id\":\"cmpl_1\",\"choices\":[{\"delta\":{\"content\":\"lo\"},\"finish_reason\":\"stop\"}],\"usage\":{\"prompt_tokens\":3,\"completion_tokens\":2,\"total_tokens\":5}}\n\n\
                   data: [DONE]\n\n";

        let events = mistral_sse_text_to_stream_events(sse).expect("sse should parse");

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
            StreamEvent::Usage { usage }
                if usage.input == 3 && usage.output == 2 && usage.total_tokens == 5
        ));
        assert!(matches!(
            events.last(),
            Some(StreamEvent::RichFinished { message }) if crate::stream::rich_assistant_text(message) == "hello"
        ));
    }

    #[test]
    fn mistral_stream_preserves_response_id_like_pi() {
        let sse = "data: {\"id\":\"cmpl_resp_1\",\"choices\":[{\"delta\":{\"content\":\"hi\"},\"finish_reason\":\"stop\"}]}\n\n";

        let events = mistral_sse_text_to_stream_events(sse).expect("sse should parse");
        let stream = crate::provider_events_to_stream(events).expect("stream");

        assert_eq!(
            stream
                .result()
                .and_then(|message| message.response_id.as_deref()),
            Some("cmpl_resp_1")
        );
    }

    #[test]
    fn mistral_stream_maps_finish_reason_like_pi() {
        let sse = "data: {\"id\":\"cmpl_resp_1\",\"choices\":[{\"delta\":{\"content\":\"hi\"},\"finish_reason\":\"tool_calls\"}]}\n\n";

        let events = mistral_sse_text_to_stream_events(sse).expect("sse should parse");
        let stream = crate::provider_events_to_stream(events).expect("stream");

        assert_eq!(
            stream.result().map(|message| message.stop_reason.clone()),
            Some(AssistantStopReason::ToolUse)
        );
    }

    #[test]
    fn mistral_stream_accumulates_tool_calls_like_pi() {
        let sse = "data: {\"id\":\"cmpl_tool_1\",\"choices\":[{\"delta\":{\"toolCalls\":[{\"index\":0,\"id\":\"call_read\",\"function\":{\"name\":\"read\",\"arguments\":\"{\\\"path\\\":\\\"README\"}}]},\"finish_reason\":null}]}\n\n\
                   data: {\"id\":\"cmpl_tool_1\",\"choices\":[{\"delta\":{\"toolCalls\":[{\"index\":0,\"id\":\"call_read\",\"function\":{\"arguments\":\".md\\\"}\"}}]},\"finish_reason\":\"tool_calls\"}]}\n\n";

        let events = mistral_sse_text_to_stream_events(sse).expect("sse should parse");

        assert!(matches!(
            &events[0],
            StreamEvent::ToolCallStart { content_index } if *content_index == 0
        ));
        assert!(matches!(
            &events[1],
            StreamEvent::ToolCallDelta {
                content_index,
                delta,
            } if *content_index == 0 && delta == "{\"path\":\"README"
        ));
        assert!(matches!(
            &events[2],
            StreamEvent::ToolCallDelta {
                content_index,
                delta,
            } if *content_index == 0 && delta == ".md\"}"
        ));
        assert!(matches!(
            &events[3],
            StreamEvent::ToolCallEnd {
                content_index,
                tool_call,
            } if *content_index == 0
                && tool_call.id == "call_read"
                && tool_call.name == "read"
                && tool_call.arguments["path"] == serde_json::json!("README.md")
        ));

        let stream = crate::provider_events_to_stream(events).expect("stream");
        assert_eq!(
            stream.result().map(|message| message.stop_reason.clone()),
            Some(AssistantStopReason::ToolUse)
        );
    }

    #[test]
    fn mistral_stream_preserves_thinking_content_like_pi() {
        let sse = "data: {\"id\":\"cmpl_think_1\",\"choices\":[{\"delta\":{\"content\":[{\"type\":\"thinking\",\"thinking\":[{\"type\":\"text\",\"text\":\"plan\"}]},{\"type\":\"text\",\"text\":\"answer\"}]},\"finish_reason\":\"stop\"}]}\n\n";

        let events = mistral_sse_text_to_stream_events(sse).expect("sse should parse");

        assert!(matches!(
            &events[0],
            StreamEvent::ThinkingStart { content_index } if *content_index == 0
        ));
        assert!(matches!(
            &events[1],
            StreamEvent::ThinkingDelta {
                content_index,
                delta,
            } if *content_index == 0 && delta == "plan"
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

        let stream = crate::provider_events_to_stream(events).expect("stream");
        let message = stream.result().expect("final message");
        assert_eq!(message.content, "answer");
        assert!(matches!(
            &message.content_blocks[0],
            AssistantContentBlock::Thinking(thinking) if thinking.thinking == "plan"
        ));
    }

    #[test]
    fn builds_mistral_tools_with_nested_schema_like_pi() {
        let request = StreamRequest {
            model: crate::types::Model {
                id: "devstral-medium-latest".to_string(),
                provider: "mistral".to_string(),
                api: "mistral-chat-completions".to_string(),
                display_name: "Devstral Medium".to_string(),
                ..crate::types::Model::default()
            },
            messages: vec![crate::types::Message {
                role: MessageRole::User,
                content: "Hi".to_string(),
            }],
            rich_messages: Vec::new(),
            tools: vec![crate::types::ToolDefinition {
                name: "inspect_schema".to_string(),
                description: "Inspect the schema".to_string(),
                parameters: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "nested": {
                            "type": "object",
                            "properties": {
                                "value": { "type": "string" }
                            }
                        }
                    }
                }),
            }],
            metadata: Default::default(),
        };

        let payload = MistralChatRequest::from_stream_request(request);

        let tools = payload.tools.expect("tools");
        assert_eq!(tools.len(), 1);
        assert_eq!(tools[0].r#type, "function");
        assert_eq!(tools[0].function.name, "inspect_schema");
        assert_eq!(
            tools[0].function.parameters["properties"]["nested"]["properties"]["value"]["type"],
            "string"
        );
    }
}
