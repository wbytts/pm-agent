use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::env;

use crate::conversation::{AssistantContentBlock, RichAssistantMessage, TextContent};
use crate::providers::chat_role;
use crate::types::{
    validate_model, AiError, AiResult, AssistantStopReason, LanguageModelProvider,
    ModelThinkingLevel, StreamEvent, StreamRequest, ToolDefinition, Usage,
};

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

    for chunk in chunks {
        if response_id.is_none() {
            response_id = chunk.id.clone().filter(|value| !value.is_empty());
        }
        if let Some(choice) = chunk.choices.into_iter().next() {
            if choice.finish_reason.is_some() {
                saw_finish = true;
            }
            if let Some(delta) = choice.delta {
                for text in mistral_delta_texts(delta.content) {
                    if text.is_empty() {
                        continue;
                    }
                    content.push_str(&text);
                    events.push(StreamEvent::TextDelta { text });
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

    if content.is_empty() {
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
            stop_reason: AssistantStopReason::Stop,
            error_message: None,
            diagnostics: Vec::new(),
            timestamp_millis: 0,
        },
    });
    Ok(events)
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

fn mistral_delta_texts(content: Option<Value>) -> Vec<String> {
    match content {
        Some(Value::String(text)) => vec![text],
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
        _ => Vec::new(),
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
