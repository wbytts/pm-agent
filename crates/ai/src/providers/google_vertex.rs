use std::env;

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::conversation::{RichMessage, TextContent, UserContentBlock, UserMessageContent};
use crate::providers::{
    build_base_options, clamp_reasoning, convert_google_messages, convert_google_tools,
    google_sse_text_to_stream_events, map_google_tool_choice, GoogleContent, GoogleMessagesContext,
    GoogleThinkingLevel, GoogleToolChoiceMode, GoogleToolDefinition, SimpleStreamOptions,
    ThinkingBudgets,
};
use crate::types::{
    validate_model, AiError, AiResult, LanguageModelProvider, Message, MessageRole, Model,
    ModelThinkingLevel, StreamEvent, StreamRequest,
};
use crate::utils::sanitize_surrogates;

pub const GOOGLE_VERTEX_API_VERSION: &str = "v1";
pub const GCP_VERTEX_CREDENTIALS_MARKER: &str = "gcp-vertex-credentials";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct GoogleVertexConfig {
    pub api_key: Option<String>,
    pub project: Option<String>,
    pub location: Option<String>,
    pub base_url: Option<String>,
}

#[derive(Debug, Clone)]
pub struct GoogleVertexProvider {
    config: GoogleVertexConfig,
}

impl GoogleVertexProvider {
    pub fn new(config: GoogleVertexConfig) -> Self {
        Self { config }
    }

    pub fn from_env() -> Self {
        Self::new(GoogleVertexConfig {
            api_key: env::var("GOOGLE_CLOUD_API_KEY").ok(),
            project: env::var("GOOGLE_CLOUD_PROJECT")
                .or_else(|_| env::var("GCLOUD_PROJECT"))
                .ok(),
            location: env::var("GOOGLE_CLOUD_LOCATION").ok(),
            base_url: env::var("GOOGLE_VERTEX_BASE_URL").ok(),
        })
    }

    fn resolve_credentials(&self) -> AiResult<GoogleVertexCredentials> {
        let api_key = resolve_google_vertex_api_key(self.config.api_key.as_deref());
        if let Some(api_key) = api_key {
            return Ok(GoogleVertexCredentials::ApiKey(api_key));
        }

        let project = self
            .config
            .project
            .as_deref()
            .filter(|value| !value.is_empty());
        let location = self
            .config
            .location
            .as_deref()
            .filter(|value| !value.is_empty());
        match (project, location) {
            (Some(project), Some(location)) => Ok(GoogleVertexCredentials::Adc {
                project: project.to_string(),
                location: location.to_string(),
            }),
            (None, _) => Err(AiError::MissingApiKey(
                "GOOGLE_CLOUD_API_KEY 或 GOOGLE_CLOUD_PROJECT/GCLOUD_PROJECT".to_string(),
            )),
            (_, None) => Err(AiError::MissingApiKey("GOOGLE_CLOUD_LOCATION".to_string())),
        }
    }

    fn stream_with_access_token(
        &self,
        request: StreamRequest,
        access_token: Option<&str>,
    ) -> AiResult<Vec<StreamEvent>> {
        validate_model(&request.model)?;
        let credentials = self.resolve_credentials()?;
        let rich_messages = request.rich_messages;
        let (system_prompt, messages) = split_vertex_system_messages(request.messages);
        let context_messages = if rich_messages.is_empty() {
            messages
                .into_iter()
                .map(simple_message_to_google_rich_message)
                .collect()
        } else {
            rich_messages
        };
        let context = GoogleMessagesContext {
            messages: context_messages,
        };
        let params = build_google_vertex_params(
            &request.model,
            &GoogleVertexContext {
                system_prompt,
                messages: context.messages,
                tools: request
                    .tools
                    .into_iter()
                    .map(|tool| GoogleToolDefinition {
                        name: tool.name,
                        description: tool.description,
                        parameters: tool.parameters,
                    })
                    .collect(),
            },
            &GoogleVertexOptions::default(),
        );
        let payload = google_vertex_rest_payload(&params).map_err(AiError::InvalidResponse)?;
        let client = reqwest::blocking::Client::new();
        let response = match credentials {
            GoogleVertexCredentials::ApiKey(api_key) => {
                let url = build_google_vertex_stream_url(
                    self.config.base_url.as_deref(),
                    &request.model.id,
                    &api_key,
                );
                client.post(url).json(&payload).send()
            }
            GoogleVertexCredentials::Adc { project, location } => {
                let access_token = access_token
                    .and_then(|value| resolve_google_vertex_access_token(Some(value)))
                    .or_else(|| {
                        env::var("GOOGLE_VERTEX_ACCESS_TOKEN")
                            .ok()
                            .and_then(|value| resolve_google_vertex_access_token(Some(&value)))
                    })
                    .ok_or_else(|| {
                        AiError::Http(
                            "Google Vertex ADC runtime 需要 GOOGLE_VERTEX_ACCESS_TOKEN，完整 ADC 自动刷新需要 Google SDK 适配"
                                .to_string(),
                        )
                    })?;
                let url = build_google_vertex_adc_stream_url(
                    self.config.base_url.as_deref(),
                    &project,
                    &location,
                    &request.model.id,
                );
                client.post(url).bearer_auth(access_token).json(&payload).send()
            }
        }
        .map_err(|error| AiError::Http(error.to_string()))?;
        let status = response.status();
        if !status.is_success() {
            let body = response.text().unwrap_or_default();
            return Err(AiError::Http(format!("status={status}, body={body}")));
        }

        let body = response
            .text()
            .map_err(|error| AiError::InvalidResponse(error.to_string()))?;
        google_sse_text_to_stream_events(&body).map_err(AiError::InvalidResponse)
    }

    #[cfg(test)]
    fn stream_with_access_token_for_test(
        &self,
        request: StreamRequest,
        access_token: Option<&str>,
    ) -> AiResult<Vec<StreamEvent>> {
        self.stream_with_access_token(request, access_token)
    }
}

impl LanguageModelProvider for GoogleVertexProvider {
    fn stream(&self, request: StreamRequest) -> AiResult<Vec<StreamEvent>> {
        self.stream_with_access_token(request, None)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GoogleVertexCredentials {
    ApiKey(String),
    Adc { project: String, location: String },
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct GoogleVertexOptions {
    pub api_key: Option<String>,
    pub tool_choice: Option<String>,
    pub thinking: Option<GoogleVertexThinkingOptions>,
    pub project: Option<String>,
    pub location: Option<String>,
    pub temperature: Option<f64>,
    pub max_tokens: Option<usize>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct GoogleVertexThinkingOptions {
    pub enabled: bool,
    pub budget_tokens: Option<i64>,
    pub level: Option<GoogleThinkingLevel>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct GoogleVertexGenerateContentParams {
    pub model: String,
    pub contents: Vec<GoogleContent>,
    pub config: GoogleVertexGenerateContentConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct GoogleVertexGenerateContentConfig {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_output_tokens: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub system_instruction: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tools: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_config: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub thinking_config: Option<GoogleVertexThinkingConfig>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct GoogleVertexThinkingConfig {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub include_thoughts: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub thinking_budget: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub thinking_level: Option<GoogleThinkingLevel>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct GoogleVertexHttpOptions {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub base_url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub api_version: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct GoogleVertexContext {
    pub system_prompt: Option<String>,
    pub messages: Vec<RichMessage>,
    pub tools: Vec<GoogleToolDefinition>,
}

pub fn resolve_google_vertex_api_key(api_key: Option<&str>) -> Option<String> {
    let env_api_key = env::var("GOOGLE_CLOUD_API_KEY").ok();
    let api_key = api_key.or(env_api_key.as_deref()).map(str::trim)?;
    if api_key.is_empty()
        || api_key == GCP_VERTEX_CREDENTIALS_MARKER
        || is_google_vertex_placeholder_api_key(api_key)
    {
        None
    } else {
        Some(api_key.to_string())
    }
}

pub fn resolve_google_vertex_access_token(access_token: Option<&str>) -> Option<String> {
    let token = access_token?.trim();
    (!token.is_empty() && token != "<token>").then(|| token.to_string())
}

pub fn is_google_vertex_placeholder_api_key(api_key: &str) -> bool {
    api_key.starts_with('<') && api_key.ends_with('>') && api_key.len() > 1
}

pub fn resolve_google_vertex_custom_base_url(base_url: Option<&str>) -> Option<String> {
    let trimmed = base_url?.trim();
    if trimmed.is_empty() || trimmed.contains("{location}") {
        None
    } else {
        Some(trimmed.to_string())
    }
}

pub fn google_vertex_base_url_includes_api_version(base_url: &str) -> bool {
    base_url
        .split('/')
        .any(|part| part.starts_with('v') && part[1..].chars().all(|c| c.is_ascii_digit()))
        || base_url
            .split('/')
            .any(|part| part.starts_with('v') && part.contains("beta"))
}

pub fn build_google_vertex_http_options(base_url: Option<&str>) -> Option<GoogleVertexHttpOptions> {
    let base_url = resolve_google_vertex_custom_base_url(base_url)?;
    Some(GoogleVertexHttpOptions {
        api_version: google_vertex_base_url_includes_api_version(&base_url).then(|| String::new()),
        base_url: Some(base_url),
    })
}

pub fn build_google_vertex_stream_url(
    base_url: Option<&str>,
    model_id: &str,
    api_key: &str,
) -> String {
    let model_path = if model_id.contains('/') {
        model_id.to_string()
    } else {
        format!("publishers/google/models/{model_id}")
    };
    format!(
        "{}/{}:streamGenerateContent?alt=sse&key={}",
        base_url
            .and_then(|value| resolve_google_vertex_custom_base_url(Some(value)))
            .unwrap_or_else(|| format!(
                "https://aiplatform.googleapis.com/{GOOGLE_VERTEX_API_VERSION}"
            ))
            .trim_end_matches('/'),
        model_path,
        api_key
    )
}

pub fn build_google_vertex_adc_stream_url(
    base_url: Option<&str>,
    project: &str,
    location: &str,
    model_id: &str,
) -> String {
    let base = base_url
        .and_then(|value| resolve_google_vertex_custom_base_url(Some(value)))
        .or_else(|| env::var("GOOGLE_VERTEX_BASE_URL").ok())
        .and_then(|value| resolve_google_vertex_custom_base_url(Some(&value)))
        .unwrap_or_else(|| format!("https://aiplatform.googleapis.com/{GOOGLE_VERTEX_API_VERSION}"))
        .trim_end_matches('/')
        .to_string();
    let model_path = if model_id.starts_with("projects/") {
        model_id.to_string()
    } else if model_id.starts_with("publishers/") {
        format!("projects/{project}/locations/{location}/{model_id}")
    } else {
        format!("projects/{project}/locations/{location}/publishers/google/models/{model_id}")
    };
    format!("{base}/{model_path}:streamGenerateContent?alt=sse")
}

pub fn google_vertex_rest_payload(
    params: &GoogleVertexGenerateContentParams,
) -> Result<Value, String> {
    let mut object = serde_json::Map::new();
    object.insert(
        "contents".to_string(),
        serde_json::to_value(&params.contents)
            .map_err(|error| format!("Vertex contents JSON 无效：{error}"))?,
    );

    let mut generation_config = serde_json::Map::new();
    if let Some(temperature) = params.config.temperature {
        generation_config.insert("temperature".to_string(), json!(temperature));
    }
    if let Some(max_output_tokens) = params.config.max_output_tokens {
        generation_config.insert("maxOutputTokens".to_string(), json!(max_output_tokens));
    }
    if let Some(thinking_config) = &params.config.thinking_config {
        generation_config.insert(
            "thinkingConfig".to_string(),
            serde_json::to_value(thinking_config)
                .map_err(|error| format!("Vertex thinkingConfig JSON 无效：{error}"))?,
        );
    }
    if !generation_config.is_empty() {
        object.insert(
            "generationConfig".to_string(),
            Value::Object(generation_config),
        );
    }

    if let Some(system_instruction) = params.config.system_instruction.as_deref() {
        object.insert(
            "systemInstruction".to_string(),
            json!({ "parts": [{ "text": system_instruction }] }),
        );
    }
    if let Some(tools) = &params.config.tools {
        object.insert("tools".to_string(), tools.clone());
    }
    if let Some(tool_config) = &params.config.tool_config {
        object.insert("toolConfig".to_string(), tool_config.clone());
    }

    Ok(Value::Object(object))
}

fn split_vertex_system_messages(messages: Vec<Message>) -> (Option<String>, Vec<Message>) {
    let mut system = Vec::new();
    let mut rest = Vec::new();
    for message in messages {
        if message.role == MessageRole::System {
            system.push(message.content);
        } else {
            rest.push(message);
        }
    }
    let system_prompt = if system.is_empty() {
        None
    } else {
        Some(system.join("\n\n"))
    };
    (system_prompt, rest)
}

pub fn build_google_vertex_params(
    model: &Model,
    context: &GoogleVertexContext,
    options: &GoogleVertexOptions,
) -> GoogleVertexGenerateContentParams {
    let mut config = GoogleVertexGenerateContentConfig {
        temperature: options.temperature,
        max_output_tokens: options.max_tokens,
        system_instruction: context
            .system_prompt
            .as_deref()
            .filter(|prompt| !prompt.is_empty())
            .map(sanitize_surrogates),
        tools: convert_google_tools(&context.tools, false).map(|tools| json!(tools)),
        tool_config: None,
        thinking_config: None,
    };
    if !context.tools.is_empty() {
        if let Some(tool_choice) = options.tool_choice.as_deref() {
            config.tool_config = Some(json!({
                "functionCallingConfig": {
                    "mode": google_vertex_tool_choice_mode(tool_choice),
                }
            }));
        }
    }
    config.thinking_config = build_google_vertex_thinking_config(model, options.thinking.as_ref());

    GoogleVertexGenerateContentParams {
        model: model.id.clone(),
        contents: convert_google_messages(
            model,
            &GoogleMessagesContext {
                messages: context.messages.clone(),
            },
        ),
        config,
    }
}

pub fn build_google_vertex_simple_options(
    model: &Model,
    options: Option<&SimpleStreamOptions>,
) -> GoogleVertexOptions {
    let base = build_base_options(model, options.cloned(), None);
    let Some(options) = options else {
        return GoogleVertexOptions {
            thinking: model
                .reasoning
                .as_ref()
                .map(|_| GoogleVertexThinkingOptions {
                    enabled: false,
                    budget_tokens: None,
                    level: None,
                }),
            temperature: base.temperature,
            max_tokens: base.max_tokens,
            ..GoogleVertexOptions::default()
        };
    };
    let reasoning = options
        .metadata
        .get("reasoning")
        .and_then(Value::as_str)
        .and_then(parse_google_vertex_thinking_level);
    let Some(reasoning) = reasoning else {
        return GoogleVertexOptions {
            thinking: Some(GoogleVertexThinkingOptions {
                enabled: false,
                budget_tokens: None,
                level: None,
            }),
            temperature: base.temperature,
            max_tokens: base.max_tokens,
            ..GoogleVertexOptions::default()
        };
    };
    let custom_budgets = options
        .metadata
        .get("thinkingBudgets")
        .and_then(parse_google_vertex_thinking_budgets);
    let clamped = clamp_reasoning(Some(reasoning)).unwrap_or(ModelThinkingLevel::High);
    GoogleVertexOptions {
        thinking: Some(
            if is_gemini_3_pro_model(&model.id) || is_gemini_3_flash_model(&model.id) {
                GoogleVertexThinkingOptions {
                    enabled: true,
                    budget_tokens: None,
                    level: Some(google_vertex_gemini_3_thinking_level(&model.id, clamped)),
                }
            } else {
                GoogleVertexThinkingOptions {
                    enabled: true,
                    budget_tokens: Some(google_vertex_budget_for_level(
                        &model.id,
                        clamped,
                        custom_budgets,
                    )),
                    level: None,
                }
            },
        ),
        temperature: base.temperature,
        max_tokens: base.max_tokens,
        ..GoogleVertexOptions::default()
    }
}

fn parse_google_vertex_thinking_level(value: &str) -> Option<ModelThinkingLevel> {
    match value {
        "off" => Some(ModelThinkingLevel::Off),
        "minimal" => Some(ModelThinkingLevel::Minimal),
        "low" => Some(ModelThinkingLevel::Low),
        "medium" => Some(ModelThinkingLevel::Medium),
        "high" => Some(ModelThinkingLevel::High),
        "xhigh" => Some(ModelThinkingLevel::XHigh),
        _ => None,
    }
}

fn parse_google_vertex_thinking_budgets(value: &Value) -> Option<ThinkingBudgets> {
    let object = value.as_object()?;
    Some(ThinkingBudgets {
        minimal: object.get("minimal")?.as_u64()? as usize,
        low: object.get("low")?.as_u64()? as usize,
        medium: object.get("medium")?.as_u64()? as usize,
        high: object.get("high")?.as_u64()? as usize,
    })
}

pub fn build_google_vertex_thinking_config(
    model: &Model,
    thinking: Option<&GoogleVertexThinkingOptions>,
) -> Option<GoogleVertexThinkingConfig> {
    let thinking = thinking?;
    if thinking.enabled && model.reasoning.is_some() {
        Some(GoogleVertexThinkingConfig {
            include_thoughts: Some(true),
            thinking_budget: thinking.budget_tokens,
            thinking_level: thinking.level,
        })
    } else if !thinking.enabled && model.reasoning.is_some() {
        Some(disabled_google_vertex_thinking_config(&model.id))
    } else {
        None
    }
}

pub fn disabled_google_vertex_thinking_config(model_id: &str) -> GoogleVertexThinkingConfig {
    if is_gemini_3_pro_model(model_id) {
        return GoogleVertexThinkingConfig {
            include_thoughts: None,
            thinking_budget: None,
            thinking_level: Some(GoogleThinkingLevel::Low),
        };
    }
    if is_gemini_3_flash_model(model_id) {
        return GoogleVertexThinkingConfig {
            include_thoughts: None,
            thinking_budget: None,
            thinking_level: Some(GoogleThinkingLevel::Minimal),
        };
    }
    GoogleVertexThinkingConfig {
        include_thoughts: None,
        thinking_budget: Some(0),
        thinking_level: None,
    }
}

pub fn google_vertex_budget_for_level(
    model_id: &str,
    level: ModelThinkingLevel,
    custom_budgets: Option<ThinkingBudgets>,
) -> i64 {
    if let Some(custom) = custom_budgets {
        match level {
            ModelThinkingLevel::Minimal => return custom.minimal as i64,
            ModelThinkingLevel::Low => return custom.low as i64,
            ModelThinkingLevel::Medium => return custom.medium as i64,
            ModelThinkingLevel::High | ModelThinkingLevel::XHigh => return custom.high as i64,
            ModelThinkingLevel::Off => {}
        }
    }
    if model_id.contains("2.5-pro") {
        return match level {
            ModelThinkingLevel::Minimal => 128,
            ModelThinkingLevel::Low => 2048,
            ModelThinkingLevel::Medium => 8192,
            ModelThinkingLevel::High | ModelThinkingLevel::XHigh => 32768,
            ModelThinkingLevel::Off => -1,
        };
    }
    if model_id.contains("2.5-flash") {
        return match level {
            ModelThinkingLevel::Minimal => 128,
            ModelThinkingLevel::Low => 2048,
            ModelThinkingLevel::Medium => 8192,
            ModelThinkingLevel::High | ModelThinkingLevel::XHigh => 24576,
            ModelThinkingLevel::Off => -1,
        };
    }
    -1
}

pub fn google_vertex_gemini_3_thinking_level(
    model_id: &str,
    level: ModelThinkingLevel,
) -> GoogleThinkingLevel {
    if is_gemini_3_pro_model(model_id) {
        match level {
            ModelThinkingLevel::Minimal | ModelThinkingLevel::Low => GoogleThinkingLevel::Low,
            ModelThinkingLevel::Medium | ModelThinkingLevel::High | ModelThinkingLevel::XHigh => {
                GoogleThinkingLevel::High
            }
            ModelThinkingLevel::Off => GoogleThinkingLevel::High,
        }
    } else {
        match level {
            ModelThinkingLevel::Minimal => GoogleThinkingLevel::Minimal,
            ModelThinkingLevel::Low => GoogleThinkingLevel::Low,
            ModelThinkingLevel::Medium => GoogleThinkingLevel::Medium,
            ModelThinkingLevel::High | ModelThinkingLevel::XHigh => GoogleThinkingLevel::High,
            ModelThinkingLevel::Off => GoogleThinkingLevel::High,
        }
    }
}

fn google_vertex_tool_choice_mode(choice: &str) -> &'static str {
    match map_google_tool_choice(choice) {
        GoogleToolChoiceMode::Auto => "AUTO",
        GoogleToolChoiceMode::None => "NONE",
        GoogleToolChoiceMode::Any => "ANY",
    }
}

fn is_gemini_3_pro_model(model_id: &str) -> bool {
    let lower = model_id.to_ascii_lowercase();
    lower.starts_with("gemini-3") && lower.contains("-pro")
}

fn is_gemini_3_flash_model(model_id: &str) -> bool {
    let lower = model_id.to_ascii_lowercase();
    lower.starts_with("gemini-3") && lower.contains("-flash")
}

fn simple_message_to_google_rich_message(message: Message) -> RichMessage {
    match message.role {
        MessageRole::Assistant => {
            RichMessage::Assistant(crate::conversation::RichAssistantMessage {
                content: vec![crate::conversation::AssistantContentBlock::Text(
                    TextContent {
                        text: message.content,
                        text_signature: None,
                    },
                )],
                api: "google-vertex".to_string(),
                provider: "google-vertex".to_string(),
                model: String::new(),
                response_model: None,
                response_id: None,
                usage: crate::types::Usage::default(),
                stop_reason: crate::types::AssistantStopReason::Stop,
                error_message: None,
                diagnostics: Vec::new(),
                timestamp_millis: 0,
            })
        }
        MessageRole::System | MessageRole::User => {
            RichMessage::User(crate::conversation::UserMessage {
                content: UserMessageContent::Text(message.content),
                timestamp_millis: 0,
            })
        }
        MessageRole::Tool => RichMessage::ToolResult(crate::conversation::ToolResultMessage {
            tool_call_id: "tool".to_string(),
            tool_name: "tool".to_string(),
            content: vec![UserContentBlock::Text(TextContent {
                text: message.content,
                text_signature: None,
            })],
            details: None,
            is_error: false,
            timestamp_millis: 0,
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::conversation::{ImageContent, RichMessage, UserMessage};
    use crate::types::{LanguageModelProvider, ModelInputKind, ModelReasoning};
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::thread;

    #[test]
    fn filters_placeholder_api_keys() {
        assert_eq!(
            resolve_google_vertex_api_key(Some(GCP_VERTEX_CREDENTIALS_MARKER)),
            None
        );
        assert_eq!(resolve_google_vertex_api_key(Some("<key>")), None);
        assert_eq!(
            resolve_google_vertex_api_key(Some(" vertex-key ")).as_deref(),
            Some("vertex-key")
        );
    }

    #[test]
    fn builds_http_options_for_custom_base_url() {
        assert_eq!(build_google_vertex_http_options(None), None);
        assert_eq!(
            build_google_vertex_http_options(Some("https://example.com/v1"))
                .expect("options")
                .api_version
                .as_deref(),
            Some("")
        );
        assert_eq!(
            build_google_vertex_http_options(Some("https://example.com/{location}")),
            None
        );
    }

    #[test]
    fn builds_vertex_api_key_stream_url_for_express_mode_like_pi() {
        assert_eq!(
            build_google_vertex_stream_url(None, "gemini-2.5-pro", "vertex-key"),
            "https://aiplatform.googleapis.com/v1/publishers/google/models/gemini-2.5-pro:streamGenerateContent?alt=sse&key=vertex-key"
        );
        assert_eq!(
            build_google_vertex_stream_url(
                Some("https://example.test/v1/"),
                "publishers/acme/models/custom",
                "vertex-key",
            ),
            "https://example.test/v1/publishers/acme/models/custom:streamGenerateContent?alt=sse&key=vertex-key"
        );
    }

    #[test]
    fn builds_vertex_rest_payload_from_genai_params_like_pi() {
        let model = reasoning_model("gemini-2.5-pro");
        let params = build_google_vertex_params(
            &model,
            &GoogleVertexContext {
                system_prompt: Some("system".to_string()),
                messages: vec![RichMessage::User(UserMessage {
                    content: UserMessageContent::Text("hello".to_string()),
                    timestamp_millis: 1,
                })],
                tools: Vec::new(),
            },
            &GoogleVertexOptions {
                thinking: Some(GoogleVertexThinkingOptions {
                    enabled: true,
                    budget_tokens: Some(2048),
                    level: None,
                }),
                temperature: Some(0.4),
                max_tokens: Some(100),
                ..GoogleVertexOptions::default()
            },
        );

        let payload = google_vertex_rest_payload(&params).expect("payload");

        assert_eq!(payload["contents"][0]["role"], "user");
        assert_eq!(payload["contents"][0]["parts"][0]["text"], "hello");
        assert_eq!(payload["systemInstruction"]["parts"][0]["text"], "system");
        assert_eq!(payload["generationConfig"]["temperature"], json!(0.4));
        assert_eq!(payload["generationConfig"]["maxOutputTokens"], json!(100));
        assert_eq!(
            payload["generationConfig"]["thinkingConfig"]["thinkingBudget"],
            json!(2048)
        );
    }

    #[test]
    fn maps_disabled_thinking_like_vertex_provider() {
        assert_eq!(
            disabled_google_vertex_thinking_config("gemini-3-pro")
                .thinking_level
                .expect("level"),
            GoogleThinkingLevel::Low
        );
        assert_eq!(
            disabled_google_vertex_thinking_config("gemini-3-flash")
                .thinking_level
                .expect("level"),
            GoogleThinkingLevel::Minimal
        );
        assert_eq!(
            disabled_google_vertex_thinking_config("gemini-2.5-pro").thinking_budget,
            Some(0)
        );
    }

    #[test]
    fn maps_gemini_three_and_budget_thinking() {
        assert_eq!(
            google_vertex_gemini_3_thinking_level("gemini-3-pro", ModelThinkingLevel::Medium),
            GoogleThinkingLevel::High
        );
        assert_eq!(
            google_vertex_gemini_3_thinking_level("gemini-3-flash", ModelThinkingLevel::Minimal),
            GoogleThinkingLevel::Minimal
        );
        assert_eq!(
            google_vertex_budget_for_level("gemini-2.5-pro", ModelThinkingLevel::High, None),
            32768
        );
        assert_eq!(
            google_vertex_budget_for_level("gemini-2.5-flash", ModelThinkingLevel::High, None),
            24576
        );
    }

    #[test]
    fn builds_vertex_params_with_tools_and_thinking() {
        let model = reasoning_model("gemini-2.5-pro");
        let context = GoogleVertexContext {
            system_prompt: Some("system".to_string()),
            messages: vec![RichMessage::User(UserMessage {
                content: UserMessageContent::Text("hello".to_string()),
                timestamp_millis: 1,
            })],
            tools: vec![GoogleToolDefinition {
                name: "read_file".to_string(),
                description: "读取文件".to_string(),
                parameters: json!({"type": "object"}),
            }],
        };

        let params = build_google_vertex_params(
            &model,
            &context,
            &GoogleVertexOptions {
                tool_choice: Some("any".to_string()),
                thinking: Some(GoogleVertexThinkingOptions {
                    enabled: true,
                    budget_tokens: Some(8192),
                    level: None,
                }),
                temperature: Some(0.2),
                max_tokens: Some(1024),
                ..GoogleVertexOptions::default()
            },
        );

        assert_eq!(params.model, "gemini-2.5-pro");
        assert_eq!(params.config.system_instruction.as_deref(), Some("system"));
        assert_eq!(params.config.temperature, Some(0.2));
        assert_eq!(
            params.config.tool_config.expect("tool config")["functionCallingConfig"]["mode"],
            "ANY"
        );
        assert_eq!(
            params
                .config
                .thinking_config
                .expect("thinking")
                .thinking_budget,
            Some(8192)
        );
    }

    #[test]
    fn simple_options_choose_vertex_thinking_shape() {
        let mut options = SimpleStreamOptions::default();
        options
            .metadata
            .insert("reasoning".to_string(), json!("high"));

        let gemini_three =
            build_google_vertex_simple_options(&reasoning_model("gemini-3-pro"), Some(&options));
        assert_eq!(
            gemini_three.thinking.expect("thinking").level,
            Some(GoogleThinkingLevel::High)
        );

        let gemini_two = build_google_vertex_simple_options(
            &reasoning_model("gemini-2.5-flash"),
            Some(&options),
        );
        assert_eq!(
            gemini_two.thinking.expect("thinking").budget_tokens,
            Some(24576)
        );
    }

    #[test]
    fn vertex_api_key_runtime_posts_rest_payload_and_parses_sse_like_pi() {
        let listener = TcpListener::bind("127.0.0.1:0").expect("listener");
        let address = listener.local_addr().expect("address");
        let handle = thread::spawn(move || {
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
            let header_end = request
                .windows(4)
                .position(|window| window == b"\r\n\r\n")
                .expect("headers")
                + 4;
            let headers = String::from_utf8_lossy(&request[..header_end]);
            let content_length = headers
                .lines()
                .find_map(|line| {
                    line.strip_prefix("content-length: ")
                        .or_else(|| line.strip_prefix("Content-Length: "))
                })
                .and_then(|value| value.trim().parse::<usize>().ok())
                .expect("content length");
            while request.len() < header_end + content_length {
                let read = stream.read(&mut buffer).expect("read body");
                if read == 0 {
                    break;
                }
                request.extend_from_slice(&buffer[..read]);
            }
            let request_text = String::from_utf8_lossy(&request).to_string();
            assert!(request_text.starts_with(
                "POST /v1/publishers/google/models/gemini-2.5-pro:streamGenerateContent?alt=sse&key=vertex-key HTTP/1.1"
            ));
            assert!(request_text.contains("\"systemInstruction\""));
            assert!(request_text.contains("\"contents\""));
            assert!(request_text.contains("\"text\":\"hello\""));

            let body =
                "data: {\"candidates\":[{\"content\":{\"role\":\"model\",\"parts\":[{\"text\":\"hi\"}]}}],\
                 \"usageMetadata\":{\"promptTokenCount\":3,\"candidatesTokenCount\":2,\"totalTokenCount\":5}}\n\n";
            let response = format!(
                "HTTP/1.1 200 OK\r\ncontent-type: text/event-stream\r\ncontent-length: {}\r\n\r\n{}",
                body.len(),
                body
            );
            stream.write_all(response.as_bytes()).expect("write");
        });

        let provider = GoogleVertexProvider::new(GoogleVertexConfig {
            api_key: Some("vertex-key".to_string()),
            project: None,
            location: None,
            base_url: Some(format!("http://{address}/v1")),
        });
        let events = provider
            .stream(StreamRequest {
                model: reasoning_model("gemini-2.5-pro"),
                messages: vec![
                    Message {
                        role: MessageRole::System,
                        content: "system".to_string(),
                    },
                    Message {
                        role: MessageRole::User,
                        content: "hello".to_string(),
                    },
                ],
                rich_messages: Vec::new(),
                tools: Vec::new(),
                metadata: Default::default(),
            })
            .expect("stream");
        handle.join().expect("server");

        assert!(matches!(
            &events[0],
            StreamEvent::TextDelta { text } if text == "hi"
        ));
        assert!(matches!(
            &events[1],
            StreamEvent::Usage { usage } if usage.input == 3 && usage.output == 2
        ));
        assert!(matches!(
            events.last(),
            Some(StreamEvent::Finished { message }) if message.content == "hi"
        ));
    }

    #[test]
    fn vertex_runtime_prefers_stream_request_rich_messages_like_pi() {
        let listener = TcpListener::bind("127.0.0.1:0").expect("listener");
        let address = listener.local_addr().expect("address");
        let handle = thread::spawn(move || {
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
            let header_end = request
                .windows(4)
                .position(|window| window == b"\r\n\r\n")
                .expect("headers")
                + 4;
            let headers = String::from_utf8_lossy(&request[..header_end]);
            let content_length = headers
                .lines()
                .find_map(|line| {
                    line.strip_prefix("content-length: ")
                        .or_else(|| line.strip_prefix("Content-Length: "))
                })
                .and_then(|value| value.trim().parse::<usize>().ok())
                .expect("content length");
            while request.len() < header_end + content_length {
                let read = stream.read(&mut buffer).expect("read body");
                if read == 0 {
                    break;
                }
                request.extend_from_slice(&buffer[..read]);
            }
            let request_text = String::from_utf8_lossy(&request).to_string();
            assert!(!request_text.contains("fallback simple"));
            assert!(request_text.contains("\"text\":\"rich hello\""));
            assert!(request_text.contains("\"inlineData\""));
            assert!(request_text.contains("\"mimeType\":\"image/png\""));
            assert!(request_text.contains("\"data\":\"abc\""));

            let body =
                "data: {\"candidates\":[{\"content\":{\"role\":\"model\",\"parts\":[{\"text\":\"hi\"}]}}]}\n\n";
            let response = format!(
                "HTTP/1.1 200 OK\r\ncontent-type: text/event-stream\r\ncontent-length: {}\r\n\r\n{}",
                body.len(),
                body
            );
            stream.write_all(response.as_bytes()).expect("write");
        });

        let provider = GoogleVertexProvider::new(GoogleVertexConfig {
            api_key: Some("vertex-key".to_string()),
            project: None,
            location: None,
            base_url: Some(format!("http://{address}/v1")),
        });
        let mut model = reasoning_model("gemini-2.5-pro");
        model.input = vec![ModelInputKind::Image];
        let events = provider
            .stream(StreamRequest {
                model,
                messages: vec![Message {
                    role: MessageRole::User,
                    content: "fallback simple".to_string(),
                }],
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
            })
            .expect("stream");
        handle.join().expect("server");

        assert!(matches!(
            &events[0],
            StreamEvent::TextDelta { text } if text == "hi"
        ));
    }

    #[test]
    fn vertex_adc_runtime_uses_bearer_token_when_access_token_is_available_like_pi() {
        let listener = TcpListener::bind("127.0.0.1:0").expect("listener");
        let address = listener.local_addr().expect("address");
        let handle = thread::spawn(move || {
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
            let header_end = request
                .windows(4)
                .position(|window| window == b"\r\n\r\n")
                .expect("headers")
                + 4;
            let headers = String::from_utf8_lossy(&request[..header_end]);
            let content_length = headers
                .lines()
                .find_map(|line| {
                    line.strip_prefix("content-length: ")
                        .or_else(|| line.strip_prefix("Content-Length: "))
                })
                .and_then(|value| value.trim().parse::<usize>().ok())
                .expect("content length");
            while request.len() < header_end + content_length {
                let read = stream.read(&mut buffer).expect("read body");
                if read == 0 {
                    break;
                }
                request.extend_from_slice(&buffer[..read]);
            }
            let request_text = String::from_utf8_lossy(&request).to_string();
            assert!(request_text.starts_with(
                "POST /v1/projects/project-1/locations/us-central1/publishers/google/models/gemini-2.5-pro:streamGenerateContent?alt=sse HTTP/1.1"
            ));
            assert!(
                request_text.contains("authorization: Bearer oauth-token")
                    || request_text.contains("Authorization: Bearer oauth-token")
            );
            assert!(request_text.contains("\"text\":\"hello\""));

            let body = "data: {\"candidates\":[{\"content\":{\"role\":\"model\",\"parts\":[{\"text\":\"hi\"}]}}]}\n\n";
            let response = format!(
                "HTTP/1.1 200 OK\r\ncontent-type: text/event-stream\r\ncontent-length: {}\r\n\r\n{}",
                body.len(),
                body
            );
            stream.write_all(response.as_bytes()).expect("write");
        });

        let provider = GoogleVertexProvider::new(GoogleVertexConfig {
            api_key: None,
            project: Some("project-1".to_string()),
            location: Some("us-central1".to_string()),
            base_url: Some(format!("http://{address}/v1")),
        });
        let events = provider
            .stream_with_access_token_for_test(
                StreamRequest {
                    model: reasoning_model("gemini-2.5-pro"),
                    messages: vec![Message {
                        role: MessageRole::User,
                        content: "hello".to_string(),
                    }],
                    rich_messages: Vec::new(),
                    tools: Vec::new(),
                    metadata: Default::default(),
                },
                Some("oauth-token"),
            )
            .expect("stream");
        handle.join().expect("server");

        assert!(matches!(
            &events[0],
            StreamEvent::TextDelta { text } if text == "hi"
        ));
    }

    fn reasoning_model(id: &str) -> Model {
        Model {
            id: id.to_string(),
            provider: "google-vertex".to_string(),
            api: "google-vertex".to_string(),
            display_name: id.to_string(),
            context_window: 1_000_000,
            max_tokens: Some(8192),
            reasoning: Some(ModelReasoning { enabled: true }),
            cost: crate::types::ModelCost {
                input: 1.0,
                output: 1.0,
                cache_read: 0.0,
                cache_write: 0.0,
            },
            ..Model::default()
        }
    }
}
