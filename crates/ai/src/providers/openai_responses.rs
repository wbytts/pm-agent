use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;
use std::env;

use crate::conversation::RichAssistantMessage;
use crate::providers::openai_responses_shared::{
    convert_openai_responses_messages, openai_responses_stream_events_from_process_result,
    process_openai_responses_stream_events, ConvertResponsesMessagesOptions,
    OpenAiResponsesCompleted, OpenAiResponsesContent as OpenAiResponsesInputContent,
    OpenAiResponsesContext, OpenAiResponsesInputItem, OpenAiResponsesOutputItem,
    OpenAiResponsesReasoningPart, OpenAiResponsesStreamContentPart, OpenAiResponsesStreamEvent,
    OpenAiResponsesStreamOptions,
};
use crate::providers::{
    chat_role, clamp_openai_prompt_cache_key, is_cloudflare_provider,
    resolve_cloudflare_base_url_from_str,
};
use crate::types::{
    validate_model, AiError, AiResult, LanguageModelProvider, Message, MessageRole, StreamEvent,
    StreamRequest, Usage, UsageCost,
};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OpenAiResponsesConfig {
    pub api_key: Option<String>,
    pub base_url: String,
}

#[derive(Debug, Clone)]
pub struct OpenAiResponsesProvider {
    config: OpenAiResponsesConfig,
}

impl OpenAiResponsesProvider {
    pub fn new(config: OpenAiResponsesConfig) -> Self {
        Self { config }
    }

    pub fn from_env() -> Self {
        Self::new(OpenAiResponsesConfig {
            api_key: env::var("OPENAI_API_KEY").ok(),
            base_url: env::var("OPENAI_BASE_URL")
                .unwrap_or_else(|_| "https://api.openai.com/v1".to_string()),
        })
    }

    fn api_key(&self) -> AiResult<String> {
        api_key_or_missing(self.config.api_key.as_deref(), "OPENAI_API_KEY")
    }
}

impl LanguageModelProvider for OpenAiResponsesProvider {
    fn stream(&self, request: StreamRequest) -> AiResult<Vec<StreamEvent>> {
        validate_model(&request.model)?;
        let api_key = self.api_key()?;
        let raw_base_url = request
            .model
            .base_url
            .as_deref()
            .unwrap_or(&self.config.base_url);
        let base_url = responses_provider_base_url(&request.model.provider, raw_base_url)?;
        post_responses(
            responses_url(&base_url),
            Some(api_key),
            &responses_request_headers(&request),
            build_responses_payload(&request, Some(false)),
        )
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AzureOpenAiResponsesConfig {
    pub api_key: Option<String>,
    pub base_url: Option<String>,
    pub resource_name: Option<String>,
    pub api_version: String,
    pub deployment_name_map: BTreeMap<String, String>,
}

#[derive(Debug, Clone)]
pub struct AzureOpenAiResponsesProvider {
    config: AzureOpenAiResponsesConfig,
}

impl AzureOpenAiResponsesProvider {
    pub fn new(config: AzureOpenAiResponsesConfig) -> Self {
        Self { config }
    }

    pub fn from_env() -> Self {
        Self::new(AzureOpenAiResponsesConfig {
            api_key: env::var("AZURE_OPENAI_API_KEY").ok(),
            base_url: env::var("AZURE_OPENAI_BASE_URL").ok(),
            resource_name: env::var("AZURE_OPENAI_RESOURCE_NAME").ok(),
            api_version: env::var("AZURE_OPENAI_API_VERSION").unwrap_or_else(|_| "v1".to_string()),
            deployment_name_map: parse_deployment_name_map(
                env::var("AZURE_OPENAI_DEPLOYMENT_NAME_MAP").ok().as_deref(),
            ),
        })
    }

    fn api_key(&self) -> AiResult<String> {
        api_key_or_missing(self.config.api_key.as_deref(), "AZURE_OPENAI_API_KEY")
    }

    fn base_url(&self, request_base_url: Option<&str>) -> AiResult<String> {
        let raw = request_base_url
            .filter(|value| !value.trim().is_empty())
            .map(str::to_string)
            .or_else(|| self.config.base_url.clone())
            .or_else(|| {
                self.config
                    .resource_name
                    .as_ref()
                    .map(|name| format!("https://{name}.openai.azure.com/openai/v1"))
            })
            .ok_or_else(|| {
                AiError::InvalidResponse(
                    "Azure OpenAI base URL 缺失，请设置 AZURE_OPENAI_BASE_URL 或 AZURE_OPENAI_RESOURCE_NAME"
                        .to_string(),
                )
            })?;
        normalize_azure_base_url(&raw)
    }
}

impl LanguageModelProvider for AzureOpenAiResponsesProvider {
    fn stream(&self, request: StreamRequest) -> AiResult<Vec<StreamEvent>> {
        validate_model(&request.model)?;
        let api_key = self.api_key()?;
        let base_url = self.base_url(request.model.base_url.as_deref())?;
        let deployment_name = self
            .config
            .deployment_name_map
            .get(&request.model.id)
            .cloned()
            .unwrap_or_else(|| request.model.id.clone());
        let url = format!(
            "{}/responses?api-version={}",
            base_url.trim_end_matches('/'),
            self.config.api_version
        );
        post_responses(
            url,
            Some(api_key),
            &request.model.headers,
            OpenAiResponsesPayload {
                model: deployment_name,
                store: None,
                ..build_responses_payload(&request, None)
            },
        )
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OpenAiCodexResponsesConfig {
    pub api_key: Option<String>,
    pub base_url: String,
}

#[derive(Debug, Clone)]
pub struct OpenAiCodexResponsesProvider {
    config: OpenAiCodexResponsesConfig,
}

impl OpenAiCodexResponsesProvider {
    pub fn new(config: OpenAiCodexResponsesConfig) -> Self {
        Self { config }
    }

    pub fn from_env() -> Self {
        Self::new(OpenAiCodexResponsesConfig {
            api_key: env::var("OPENAI_CODEX_API_KEY")
                .or_else(|_| env::var("OPENAI_API_KEY"))
                .ok(),
            base_url: env::var("OPENAI_CODEX_BASE_URL")
                .unwrap_or_else(|_| "https://chatgpt.com/backend-api".to_string()),
        })
    }

    fn api_key(&self) -> AiResult<String> {
        api_key_or_missing(
            self.config.api_key.as_deref(),
            "OPENAI_CODEX_API_KEY 或 OPENAI_API_KEY",
        )
    }
}

impl LanguageModelProvider for OpenAiCodexResponsesProvider {
    fn stream(&self, request: StreamRequest) -> AiResult<Vec<StreamEvent>> {
        validate_model(&request.model)?;
        let api_key = self.api_key()?;
        let raw_base_url = request
            .model
            .base_url
            .as_deref()
            .unwrap_or(&self.config.base_url);
        let base_url = responses_provider_base_url(&request.model.provider, raw_base_url)?;
        post_responses(
            codex_responses_url(&base_url),
            Some(api_key),
            &responses_request_headers(&request),
            build_responses_payload(&request, Some(false)),
        )
    }
}

#[derive(Debug, Serialize)]
struct OpenAiResponsesPayload {
    model: String,
    input: Vec<OpenAiResponsesInputItem>,
    stream: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    store: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    prompt_cache_key: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    prompt_cache_retention: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    max_output_tokens: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    temperature: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    reasoning: Option<Value>,
}

#[derive(Debug, Deserialize)]
struct OpenAiResponsesResponse {
    output: Option<Vec<OpenAiResponsesOutput>>,
    output_text: Option<String>,
    usage: Option<OpenAiResponsesUsage>,
}

#[derive(Debug, Deserialize)]
struct OpenAiResponsesOutput {
    #[serde(rename = "type")]
    kind: Option<String>,
    content: Option<Vec<OpenAiResponsesContent>>,
}

#[derive(Debug, Deserialize)]
struct OpenAiResponsesContent {
    #[serde(rename = "type")]
    kind: Option<String>,
    text: Option<String>,
}

#[derive(Debug, Deserialize)]
struct OpenAiResponsesUsage {
    input_tokens: Option<u64>,
    output_tokens: Option<u64>,
    total_tokens: Option<u64>,
    input_tokens_details: Option<OpenAiInputTokenDetails>,
}

#[derive(Debug, Deserialize)]
struct OpenAiInputTokenDetails {
    cached_tokens: Option<u64>,
}

fn post_responses(
    url: String,
    api_key: Option<String>,
    headers: &BTreeMap<String, String>,
    payload: OpenAiResponsesPayload,
) -> AiResult<Vec<StreamEvent>> {
    let client = reqwest::blocking::Client::new();
    let mut request = client.post(url).json(&payload);
    if let Some(api_key) = api_key {
        request = request.bearer_auth(api_key);
    }
    for (key, value) in headers {
        request = request.header(key, value);
    }
    let response = request
        .send()
        .map_err(|error| AiError::Http(error.to_string()))?;
    let status = response.status();
    if !status.is_success() {
        let body = response.text().unwrap_or_default();
        return Err(AiError::Http(format!("status={status}, body={body}")));
    }

    if payload.stream {
        let body = response
            .text()
            .map_err(|error| AiError::InvalidResponse(error.to_string()))?;
        return responses_sse_text_to_stream_events(&body).map_err(AiError::InvalidResponse);
    }

    let response = response
        .json::<OpenAiResponsesResponse>()
        .map_err(|error| AiError::InvalidResponse(error.to_string()))?;
    let content = extract_response_text(&response)?;
    let mut events = vec![StreamEvent::TextDelta {
        text: content.clone(),
    }];
    if let Some(usage) = response.usage {
        events.push(StreamEvent::Usage {
            usage: Usage {
                input: usage.input_tokens.unwrap_or_default(),
                output: usage.output_tokens.unwrap_or_default(),
                cache_read: usage
                    .input_tokens_details
                    .and_then(|details| details.cached_tokens)
                    .unwrap_or_default(),
                cache_write: 0,
                total_tokens: usage.total_tokens.unwrap_or_default(),
                cost: UsageCost::default(),
            },
        });
    }
    events.push(StreamEvent::Finished {
        message: Message {
            role: MessageRole::Assistant,
            content,
        },
    });
    Ok(events)
}

fn build_responses_payload(request: &StreamRequest, store: Option<bool>) -> OpenAiResponsesPayload {
    OpenAiResponsesPayload {
        model: request.model.id.clone(),
        input: if request.rich_messages.is_empty() {
            convert_responses_messages(&request.messages)
        } else {
            convert_openai_responses_messages(
                &request.model,
                &OpenAiResponsesContext {
                    system_prompt: responses_system_prompt_from_messages(&request.messages),
                    messages: request.rich_messages.clone(),
                },
                ConvertResponsesMessagesOptions::default(),
            )
        },
        stream: true,
        store,
        prompt_cache_key: responses_prompt_cache_key(request),
        prompt_cache_retention: responses_prompt_cache_retention(request),
        max_output_tokens: request.model.max_tokens,
        temperature: request.metadata.get("temperature").and_then(Value::as_f64),
        reasoning: None,
    }
}

fn responses_prompt_cache_key(request: &StreamRequest) -> Option<String> {
    if responses_cache_retention(request) == Some("none") {
        return None;
    }
    let session_id = request.metadata.get("sessionId").and_then(Value::as_str);
    clamp_openai_prompt_cache_key(session_id)
}

fn responses_prompt_cache_retention(request: &StreamRequest) -> Option<String> {
    let supports_long = request
        .model
        .compat
        .get("supportsLongCacheRetention")
        .and_then(Value::as_bool)
        .unwrap_or(true);
    (supports_long && responses_cache_retention(request) == Some("long")).then(|| "24h".to_string())
}

fn responses_cache_retention(request: &StreamRequest) -> Option<&str> {
    request
        .metadata
        .get("cacheRetention")
        .and_then(Value::as_str)
}

fn responses_request_headers(request: &StreamRequest) -> BTreeMap<String, String> {
    let mut headers = request.model.headers.clone();
    if responses_cache_retention(request) == Some("none") {
        return headers;
    }

    let Some(session_id) = request.metadata.get("sessionId").and_then(Value::as_str) else {
        return headers;
    };
    let send_session_id = request
        .model
        .compat
        .get("sendSessionIdHeader")
        .and_then(Value::as_bool)
        .unwrap_or(true);
    if send_session_id {
        headers
            .entry("session_id".to_string())
            .or_insert_with(|| session_id.to_string());
    }
    headers
        .entry("x-client-request-id".to_string())
        .or_insert_with(|| session_id.to_string());
    headers
}

fn responses_system_prompt_from_messages(messages: &[Message]) -> Option<String> {
    let system_prompt = messages
        .iter()
        .filter(|message| message.role == MessageRole::System)
        .map(|message| message.content.as_str())
        .filter(|content| !content.trim().is_empty())
        .collect::<Vec<_>>()
        .join("\n\n");
    (!system_prompt.is_empty()).then_some(system_prompt)
}

fn responses_sse_text_to_stream_events(input: &str) -> Result<Vec<StreamEvent>, String> {
    let raw_events = parse_responses_sse_text(input)?;
    let processed = process_openai_responses_stream_events(
        &raw_events,
        responses_assistant_defaults(),
        &OpenAiResponsesStreamOptions::default(),
        None::<fn(&mut Usage, Option<&str>)>,
    )?;
    openai_responses_stream_events_from_process_result(processed)
}

fn responses_assistant_defaults() -> RichAssistantMessage {
    RichAssistantMessage {
        content: Vec::new(),
        api: "openai-responses".to_string(),
        provider: "openai".to_string(),
        model: String::new(),
        response_model: None,
        response_id: None,
        usage: Usage::default(),
        stop_reason: crate::types::AssistantStopReason::Stop,
        error_message: None,
        diagnostics: Vec::new(),
        timestamp_millis: 0,
    }
}

fn parse_responses_sse_text(input: &str) -> Result<Vec<OpenAiResponsesStreamEvent>, String> {
    let mut events = Vec::new();
    let mut data_lines = Vec::<String>::new();

    for line in input.lines().chain(std::iter::once("")) {
        let line = line.trim_end_matches('\r');
        if line.is_empty() {
            if !data_lines.is_empty() {
                let data = data_lines.join("\n").trim().to_string();
                data_lines.clear();
                if !data.is_empty() && data != "[DONE]" {
                    events.push(parse_responses_sse_json(&data)?);
                }
            }
            continue;
        }
        if let Some(value) = line.strip_prefix("data:") {
            data_lines.push(value.trim().to_string());
        }
    }

    Ok(events)
}

fn parse_responses_sse_json(data: &str) -> Result<OpenAiResponsesStreamEvent, String> {
    let value: Value =
        serde_json::from_str(data).map_err(|error| format!("Responses SSE JSON 无效：{error}"))?;
    let event_type = value
        .get("type")
        .and_then(Value::as_str)
        .ok_or_else(|| "Responses SSE 缺少 type".to_string())?;

    match event_type {
        "response.created" => {
            let response_id = value
                .get("response")
                .and_then(|response| response.get("id"))
                .and_then(Value::as_str)
                .or_else(|| value.get("response_id").and_then(Value::as_str))
                .unwrap_or_default()
                .to_string();
            Ok(OpenAiResponsesStreamEvent::ResponseCreated { response_id })
        }
        "response.output_item.added" => Ok(OpenAiResponsesStreamEvent::OutputItemAdded {
            item: parse_value_field::<OpenAiResponsesOutputItem>(&value, "item")?,
        }),
        "response.reasoning_summary_part.added" => {
            Ok(OpenAiResponsesStreamEvent::ReasoningSummaryPartAdded {
                part: parse_value_field::<OpenAiResponsesReasoningPart>(&value, "part")?,
            })
        }
        "response.reasoning_summary_text.delta" => {
            Ok(OpenAiResponsesStreamEvent::ReasoningSummaryTextDelta {
                delta: string_field(&value, "delta"),
            })
        }
        "response.reasoning_summary_part.done" => {
            Ok(OpenAiResponsesStreamEvent::ReasoningSummaryPartDone)
        }
        "response.reasoning_text.delta" => Ok(OpenAiResponsesStreamEvent::ReasoningTextDelta {
            delta: string_field(&value, "delta"),
        }),
        "response.content_part.added" => Ok(OpenAiResponsesStreamEvent::ContentPartAdded {
            part: parse_value_field::<OpenAiResponsesStreamContentPart>(&value, "part")?,
        }),
        "response.output_text.delta" => Ok(OpenAiResponsesStreamEvent::OutputTextDelta {
            delta: string_field(&value, "delta"),
        }),
        "response.refusal.delta" => Ok(OpenAiResponsesStreamEvent::RefusalDelta {
            delta: string_field(&value, "delta"),
        }),
        "response.function_call_arguments.delta" => {
            Ok(OpenAiResponsesStreamEvent::FunctionCallArgumentsDelta {
                delta: string_field(&value, "delta"),
            })
        }
        "response.function_call_arguments.done" => {
            Ok(OpenAiResponsesStreamEvent::FunctionCallArgumentsDone {
                arguments: string_field(&value, "arguments"),
            })
        }
        "response.output_item.done" => Ok(OpenAiResponsesStreamEvent::OutputItemDone {
            item: parse_value_field::<OpenAiResponsesOutputItem>(&value, "item")?,
        }),
        "response.completed" | "response.done" | "response.incomplete" => {
            let response = value.get("response").cloned().unwrap_or(Value::Null);
            Ok(OpenAiResponsesStreamEvent::ResponseCompleted {
                response_id: response
                    .get("id")
                    .and_then(Value::as_str)
                    .map(str::to_string),
                completed: serde_json::from_value::<OpenAiResponsesCompleted>(response)
                    .map_err(|error| format!("Responses completed 事件无效：{error}"))?,
            })
        }
        "error" => Ok(OpenAiResponsesStreamEvent::Error {
            code: value
                .get("code")
                .and_then(Value::as_str)
                .map(str::to_string),
            message: string_field(&value, "message"),
        }),
        "response.failed" => {
            let response = value.get("response");
            let error = response.and_then(|response| response.get("error"));
            let incomplete_details =
                response.and_then(|response| response.get("incomplete_details"));
            Ok(OpenAiResponsesStreamEvent::ResponseFailed {
                error_code: error
                    .and_then(|error| error.get("code"))
                    .and_then(Value::as_str)
                    .map(str::to_string),
                error_message: error
                    .and_then(|error| error.get("message"))
                    .and_then(Value::as_str)
                    .map(str::to_string),
                incomplete_reason: incomplete_details
                    .and_then(|details| details.get("reason"))
                    .and_then(Value::as_str)
                    .map(str::to_string),
            })
        }
        _ => Err(format!("未知 Responses SSE 事件：{event_type}")),
    }
}

fn parse_value_field<T>(value: &Value, field: &str) -> Result<T, String>
where
    T: for<'de> Deserialize<'de>,
{
    serde_json::from_value(
        value
            .get(field)
            .cloned()
            .ok_or_else(|| format!("Responses SSE 缺少 {field}"))?,
    )
    .map_err(|error| format!("Responses SSE {field} 无效：{error}"))
}

fn string_field(value: &Value, field: &str) -> String {
    value
        .get(field)
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string()
}

fn convert_responses_messages(messages: &[Message]) -> Vec<OpenAiResponsesInputItem> {
    messages
        .iter()
        .map(|message| OpenAiResponsesInputItem::Message {
            role: match message.role {
                MessageRole::System => "developer".to_string(),
                _ => chat_role(&message.role).to_string(),
            },
            content: vec![OpenAiResponsesInputContent::InputText {
                text: message.content.clone(),
            }],
            status: None,
            id: None,
            phase: None,
        })
        .collect()
}

fn extract_response_text(response: &OpenAiResponsesResponse) -> AiResult<String> {
    if let Some(text) = response
        .output_text
        .as_ref()
        .filter(|text| !text.is_empty())
    {
        return Ok(text.clone());
    }
    let text = response
        .output
        .as_deref()
        .unwrap_or_default()
        .iter()
        .filter(|item| item.kind.as_deref().is_none_or(|kind| kind == "message"))
        .flat_map(|item| item.content.as_deref().unwrap_or_default())
        .filter(|content| {
            content
                .kind
                .as_deref()
                .is_none_or(|kind| kind == "output_text" || kind == "text")
        })
        .filter_map(|content| content.text.clone())
        .collect::<Vec<_>>()
        .join("");
    if text.is_empty() {
        return Err(AiError::InvalidResponse(
            "Responses API 输出文本缺失".to_string(),
        ));
    }
    Ok(text)
}

fn responses_url(base_url: &str) -> String {
    let normalized = base_url.trim_end_matches('/');
    if normalized.ends_with("/responses") {
        normalized.to_string()
    } else {
        format!("{normalized}/responses")
    }
}

fn responses_provider_base_url(provider: &str, raw_base_url: &str) -> AiResult<String> {
    if is_cloudflare_provider(provider) {
        resolve_cloudflare_base_url_from_str(provider, raw_base_url)
    } else {
        Ok(raw_base_url.to_string())
    }
}

fn codex_responses_url(base_url: &str) -> String {
    let normalized = base_url.trim_end_matches('/');
    if normalized.ends_with("/codex/responses") {
        normalized.to_string()
    } else if normalized.ends_with("/codex") {
        format!("{normalized}/responses")
    } else {
        format!("{normalized}/codex/responses")
    }
}

fn normalize_azure_base_url(base_url: &str) -> AiResult<String> {
    let trimmed = base_url.trim().trim_end_matches('/');
    let Ok(mut url) = reqwest::Url::parse(trimmed) else {
        return Err(AiError::InvalidResponse(
            "Invalid Azure OpenAI base URL".to_string(),
        ));
    };
    let Some(host) = url.host_str() else {
        return Err(AiError::InvalidResponse(
            "Invalid Azure OpenAI base URL".to_string(),
        ));
    };
    let is_azure_host =
        host.ends_with(".openai.azure.com") || host.ends_with(".cognitiveservices.azure.com");
    let path = url.path().trim_end_matches('/');

    if is_azure_host && (path.is_empty() || path == "/" || path == "/openai") {
        url.set_path("/openai/v1");
        url.set_query(None);
        return Ok(url.to_string().trim_end_matches('/').to_string());
    }
    Ok(trimmed.to_string())
}

fn parse_deployment_name_map(value: Option<&str>) -> BTreeMap<String, String> {
    value
        .unwrap_or_default()
        .split(',')
        .filter_map(|entry| entry.trim().split_once('='))
        .map(|(model, deployment)| (model.trim().to_string(), deployment.trim().to_string()))
        .filter(|(model, deployment)| !model.is_empty() && !deployment.is_empty())
        .collect()
}

fn api_key_or_missing(value: Option<&str>, label: &str) -> AiResult<String> {
    let key = value.unwrap_or_default().trim();
    if key.is_empty() {
        return Err(AiError::MissingApiKey(label.to_string()));
    }
    Ok(key.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::conversation::{
        ImageContent, RichMessage, TextContent, UserContentBlock, UserMessage, UserMessageContent,
    };
    use crate::types::{Model, ModelInputKind};
    use serde_json::json;

    #[test]
    fn builds_codex_responses_url_like_pi() {
        assert_eq!(
            codex_responses_url("https://chatgpt.com/backend-api"),
            "https://chatgpt.com/backend-api/codex/responses"
        );
        assert_eq!(
            codex_responses_url("https://example.test/codex"),
            "https://example.test/codex/responses"
        );
    }

    #[test]
    fn keeps_normal_responses_base_url() {
        assert_eq!(
            responses_provider_base_url("openai", "https://api.openai.com/v1").expect("base url"),
            "https://api.openai.com/v1"
        );
    }

    #[test]
    fn accepts_resolved_cloudflare_responses_base_url() {
        assert_eq!(
            responses_provider_base_url(
                "cloudflare-ai-gateway",
                "https://gateway.ai.cloudflare.com/v1/account/gateway/openai"
            )
            .expect("base url"),
            "https://gateway.ai.cloudflare.com/v1/account/gateway/openai"
        );
    }

    #[test]
    fn normalizes_azure_base_urls_like_pi() {
        assert_eq!(
            normalize_azure_base_url("https://my-resource.cognitiveservices.azure.com")
                .expect("base url"),
            "https://my-resource.cognitiveservices.azure.com/openai/v1"
        );
        assert_eq!(
            normalize_azure_base_url(
                "https://my-resource.openai.azure.com/openai?api-version=2024-12-01"
            )
            .expect("base url"),
            "https://my-resource.openai.azure.com/openai/v1"
        );
        assert_eq!(
            normalize_azure_base_url("https://my-proxy.example.com/v1?custom=true")
                .expect("base url"),
            "https://my-proxy.example.com/v1?custom=true"
        );
    }

    #[test]
    fn rejects_invalid_azure_base_url_before_network_like_pi() {
        let provider = AzureOpenAiResponsesProvider::new(AzureOpenAiResponsesConfig {
            api_key: Some("test-api-key".to_string()),
            base_url: Some("not-a-url".to_string()),
            resource_name: None,
            api_version: "v1".to_string(),
            deployment_name_map: BTreeMap::new(),
        });
        let model = Model {
            id: "gpt-4o-mini".to_string(),
            provider: "azure-openai-responses".to_string(),
            api: "openai-responses".to_string(),
            display_name: "GPT-4o mini".to_string(),
            context_window: 128_000,
            ..Model::default()
        };

        let error = provider
            .stream(StreamRequest {
                model,
                messages: vec![Message {
                    role: MessageRole::User,
                    content: "hello".to_string(),
                }],
                rich_messages: Vec::new(),
                tools: Vec::new(),
                metadata: Default::default(),
            })
            .expect_err("invalid Azure base URL should fail before HTTP");

        assert!(matches!(
            error,
            AiError::InvalidResponse(message) if message.contains("Invalid Azure OpenAI base URL")
        ));
    }

    #[test]
    fn extracts_output_text_from_responses_shape() {
        let response: OpenAiResponsesResponse = serde_json::from_value(json!({
            "output": [{
                "type": "message",
                "content": [{"type": "output_text", "text": "hello"}]
            }]
        }))
        .expect("response should parse");
        assert_eq!(
            extract_response_text(&response).expect("text should extract"),
            "hello"
        );
    }

    #[test]
    fn remote_response_providers_require_api_key_before_network() {
        let provider = OpenAiResponsesProvider::new(OpenAiResponsesConfig {
            api_key: Some(String::new()),
            base_url: "https://api.openai.com/v1".to_string(),
        });
        let model = Model {
            id: "gpt-5".to_string(),
            provider: "openai".to_string(),
            api: "openai-responses".to_string(),
            display_name: "GPT-5".to_string(),
            context_window: 128_000,
            ..Model::default()
        };
        let error = provider
            .stream(StreamRequest {
                model,
                messages: vec![Message {
                    role: MessageRole::User,
                    content: "hello".to_string(),
                }],
                rich_messages: Vec::new(),
                tools: Vec::new(),
                metadata: Default::default(),
            })
            .expect_err("missing key should fail first");
        assert!(matches!(error, AiError::MissingApiKey(_)));
    }

    #[test]
    fn builds_streaming_responses_payload_like_pi() {
        let model = Model {
            id: "gpt-5".to_string(),
            provider: "openai".to_string(),
            api: "openai-responses".to_string(),
            display_name: "GPT-5".to_string(),
            context_window: 128_000,
            max_tokens: Some(2048),
            ..Model::default()
        };
        let request = StreamRequest {
            model,
            messages: vec![Message {
                role: MessageRole::User,
                content: "hello".to_string(),
            }],
            rich_messages: Vec::new(),
            tools: Vec::new(),
            metadata: Default::default(),
        };

        let payload = build_responses_payload(&request, Some(false));

        assert!(payload.stream);
        assert_eq!(payload.store, Some(false));
        assert_eq!(payload.max_output_tokens, Some(2048));
    }

    #[test]
    fn builds_responses_payload_prompt_cache_key_from_session_id_like_pi() {
        let model = Model {
            id: "gpt-5".to_string(),
            provider: "openai".to_string(),
            api: "openai-responses".to_string(),
            display_name: "GPT-5".to_string(),
            context_window: 128_000,
            ..Model::default()
        };
        let request = StreamRequest {
            model,
            messages: vec![Message {
                role: MessageRole::User,
                content: "hello".to_string(),
            }],
            rich_messages: Vec::new(),
            tools: Vec::new(),
            metadata: BTreeMap::from([("sessionId".to_string(), json!("x".repeat(67)))]),
        };

        let value = serde_json::to_value(build_responses_payload(&request, Some(false)))
            .expect("payload json");

        assert_eq!(value["prompt_cache_key"], "x".repeat(64));
    }

    #[test]
    fn builds_responses_payload_prompt_cache_retention_for_long_cache_like_pi() {
        let model = Model {
            id: "gpt-5".to_string(),
            provider: "openai".to_string(),
            api: "openai-responses".to_string(),
            display_name: "GPT-5".to_string(),
            context_window: 128_000,
            ..Model::default()
        };
        let request = StreamRequest {
            model,
            messages: vec![Message {
                role: MessageRole::User,
                content: "hello".to_string(),
            }],
            rich_messages: Vec::new(),
            tools: Vec::new(),
            metadata: BTreeMap::from([("cacheRetention".to_string(), json!("long"))]),
        };

        let value = serde_json::to_value(build_responses_payload(&request, Some(false)))
            .expect("payload json");

        assert_eq!(value["prompt_cache_retention"], "24h");
    }

    #[test]
    fn builds_responses_payload_temperature_from_metadata_like_pi() {
        let model = Model {
            id: "gpt-5".to_string(),
            provider: "openai".to_string(),
            api: "openai-responses".to_string(),
            display_name: "GPT-5".to_string(),
            context_window: 128_000,
            ..Model::default()
        };
        let request = StreamRequest {
            model,
            messages: vec![Message {
                role: MessageRole::User,
                content: "hello".to_string(),
            }],
            rich_messages: Vec::new(),
            tools: Vec::new(),
            metadata: BTreeMap::from([("temperature".to_string(), json!(0.3))]),
        };

        let value = serde_json::to_value(build_responses_payload(&request, Some(false)))
            .expect("payload json");

        assert_eq!(value["temperature"], json!(0.3));
    }

    #[test]
    fn omits_responses_prompt_cache_fields_when_cache_retention_is_none_like_pi() {
        let model = Model {
            id: "gpt-5".to_string(),
            provider: "openai".to_string(),
            api: "openai-responses".to_string(),
            display_name: "GPT-5".to_string(),
            context_window: 128_000,
            ..Model::default()
        };
        let request = StreamRequest {
            model,
            messages: vec![Message {
                role: MessageRole::User,
                content: "hello".to_string(),
            }],
            rich_messages: Vec::new(),
            tools: Vec::new(),
            metadata: BTreeMap::from([
                ("sessionId".to_string(), json!("session-none")),
                ("cacheRetention".to_string(), json!("none")),
            ]),
        };

        let value = serde_json::to_value(build_responses_payload(&request, Some(false)))
            .expect("payload json");

        assert!(value.get("prompt_cache_key").is_none());
        assert!(value.get("prompt_cache_retention").is_none());
    }

    #[test]
    fn omits_responses_prompt_cache_retention_when_unsupported_like_pi() {
        let model = Model {
            id: "gpt-5".to_string(),
            provider: "openai".to_string(),
            api: "openai-responses".to_string(),
            display_name: "GPT-5".to_string(),
            context_window: 128_000,
            compat: BTreeMap::from([("supportsLongCacheRetention".to_string(), json!(false))]),
            ..Model::default()
        };
        let request = StreamRequest {
            model,
            messages: vec![Message {
                role: MessageRole::User,
                content: "hello".to_string(),
            }],
            rich_messages: Vec::new(),
            tools: Vec::new(),
            metadata: BTreeMap::from([("cacheRetention".to_string(), json!("long"))]),
        };

        let value = serde_json::to_value(build_responses_payload(&request, Some(false)))
            .expect("payload json");

        assert!(value.get("prompt_cache_retention").is_none());
    }

    #[test]
    fn builds_responses_cache_affinity_headers_like_pi() {
        let model = Model {
            id: "gpt-5".to_string(),
            provider: "openai".to_string(),
            api: "openai-responses".to_string(),
            display_name: "GPT-5".to_string(),
            context_window: 128_000,
            ..Model::default()
        };
        let request = StreamRequest {
            model,
            messages: vec![Message {
                role: MessageRole::User,
                content: "hello".to_string(),
            }],
            rich_messages: Vec::new(),
            tools: Vec::new(),
            metadata: BTreeMap::from([("sessionId".to_string(), json!("session-123"))]),
        };

        let headers = responses_request_headers(&request);

        assert_eq!(
            headers.get("session_id").map(String::as_str),
            Some("session-123")
        );
        assert_eq!(
            headers.get("x-client-request-id").map(String::as_str),
            Some("session-123")
        );
    }

    #[test]
    fn can_omit_responses_session_id_header_like_pi() {
        let model = Model {
            id: "gpt-5".to_string(),
            provider: "openai".to_string(),
            api: "openai-responses".to_string(),
            display_name: "GPT-5".to_string(),
            context_window: 128_000,
            compat: BTreeMap::from([("sendSessionIdHeader".to_string(), json!(false))]),
            ..Model::default()
        };
        let request = StreamRequest {
            model,
            messages: vec![Message {
                role: MessageRole::User,
                content: "hello".to_string(),
            }],
            rich_messages: Vec::new(),
            tools: Vec::new(),
            metadata: BTreeMap::from([("sessionId".to_string(), json!("session-123"))]),
        };

        let headers = responses_request_headers(&request);

        assert!(headers.get("session_id").is_none());
        assert_eq!(
            headers.get("x-client-request-id").map(String::as_str),
            Some("session-123")
        );
    }

    #[test]
    fn omits_responses_cache_affinity_headers_when_cache_retention_is_none_like_pi() {
        let model = Model {
            id: "gpt-5".to_string(),
            provider: "openai".to_string(),
            api: "openai-responses".to_string(),
            display_name: "GPT-5".to_string(),
            context_window: 128_000,
            ..Model::default()
        };
        let request = StreamRequest {
            model,
            messages: vec![Message {
                role: MessageRole::User,
                content: "hello".to_string(),
            }],
            rich_messages: Vec::new(),
            tools: Vec::new(),
            metadata: BTreeMap::from([
                ("sessionId".to_string(), json!("session-123")),
                ("cacheRetention".to_string(), json!("none")),
            ]),
        };

        let headers = responses_request_headers(&request);

        assert!(headers.get("session_id").is_none());
        assert!(headers.get("x-client-request-id").is_none());
    }

    #[test]
    fn builds_responses_payload_prefers_rich_messages_like_pi() {
        let model = Model {
            id: "gpt-5".to_string(),
            provider: "openai".to_string(),
            api: "openai-responses".to_string(),
            display_name: "GPT-5".to_string(),
            context_window: 128_000,
            input: vec![ModelInputKind::Image],
            ..Model::default()
        };
        let request = StreamRequest {
            model,
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
            rich_messages: vec![RichMessage::User(UserMessage {
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
            })],
            tools: Vec::new(),
            metadata: Default::default(),
        };

        let payload = build_responses_payload(&request, Some(false));
        let value = serde_json::to_value(payload).expect("payload json");

        assert!(!value.to_string().contains("fallback simple"));
        assert_eq!(value["input"][0]["role"], "system");
        assert_eq!(value["input"][0]["content"][0]["text"], "system");
        assert_eq!(value["input"][1]["role"], "user");
        assert_eq!(value["input"][1]["content"][0]["text"], "rich hello");
        assert_eq!(
            value["input"][1]["content"][1]["image_url"],
            "data:image/png;base64,abc"
        );
    }

    #[test]
    fn parses_responses_sse_text_to_public_stream_events_like_pi() {
        let sse = "event: response.created\n\
                   data: {\"type\":\"response.created\",\"response\":{\"id\":\"resp_1\"}}\n\n\
                   event: response.output_item.added\n\
                   data: {\"type\":\"response.output_item.added\",\"item\":{\"type\":\"message\",\"id\":\"msg_1\",\"content\":[]}}\n\n\
                   event: response.content_part.added\n\
                   data: {\"type\":\"response.content_part.added\",\"part\":{\"type\":\"output_text\",\"text\":\"\"}}\n\n\
                   event: response.output_text.delta\n\
                   data: {\"type\":\"response.output_text.delta\",\"delta\":\"hel\"}\n\n\
                   event: response.output_text.delta\n\
                   data: {\"type\":\"response.output_text.delta\",\"delta\":\"lo\"}\n\n\
                   event: response.output_item.done\n\
                   data: {\"type\":\"response.output_item.done\",\"item\":{\"type\":\"message\",\"id\":\"msg_1\",\"content\":[{\"type\":\"output_text\",\"text\":\"hello\"}]}}\n\n\
                   event: response.completed\n\
                   data: {\"type\":\"response.completed\",\"response\":{\"id\":\"resp_1\",\"status\":\"completed\",\"usage\":{\"input_tokens\":3,\"output_tokens\":2,\"total_tokens\":5,\"input_tokens_details\":{\"cached_tokens\":1}}}}\n\n\
                   data: [DONE]\n\n";

        let events = responses_sse_text_to_stream_events(sse).expect("sse should parse");
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
                if usage.input == 2
                    && usage.cache_read == 1
                    && usage.output == 2
                    && usage.total_tokens == 5
        ));
        assert!(matches!(
            events.last(),
            Some(StreamEvent::Finished { message }) if message.content == "hello"
        ));
    }
}
